//! Power mode (power-profiles-daemon) and battery (UPower).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use zbus::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::bus::{Bus, Emit, Props, describe_error};

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";
/// power-profiles-daemon's current name, and its old one.
const PROFILES: [(&str, &str); 2] = [
    ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    NotCharging,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub percent: f64,
    pub charge: Charge,
    /// Seconds until empty (discharging) or full (charging), when UPower knows.
    pub time_left: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PowerState {
    /// Available power modes (`power-saver`, `balanced`, `performance`); empty without
    /// power-profiles-daemon.
    pub profiles: Vec<String>,
    pub active: Option<String>,
    /// Why performance mode is held back (e.g. `lap-detected`, `high-operating-temperature`).
    pub degraded: Option<String>,
    pub battery: Option<Battery>,
}

#[derive(Debug)]
pub enum Request {
    SetProfile(String),
}

#[derive(Debug)]
pub enum Event {
    State(PowerState),
    Error(String),
}

pub async fn run(mut requests: UnboundedReceiver<Request>, emit: impl Fn(Event) + Send + Sync + 'static) {
    let emit: Emit<Event> = Arc::new(emit);
    let conn = match Connection::system().await {
        Ok(conn) => conn,
        Err(e) => {
            emit(Event::State(PowerState::default()));
            emit(Event::Error(format!("No system bus: {}", describe_error(&e))));
            while requests.recv().await.is_some() {}
            return;
        }
    };
    let upower = Bus::new(conn.clone(), UPOWER);
    let mut profiles = (Bus::new(conn.clone(), PROFILES[0].0), PROFILES[0].1);
    if profiles.0.props(profiles.1, PROFILES[0].0).await.is_err() {
        profiles = (Bus::new(conn, PROFILES[1].0), PROFILES[1].1);
    }
    let iface = if profiles.1 == PROFILES[0].1 { PROFILES[0].0 } else { PROFILES[1].0 };

    let watch_upower = upower.signals().await.ok();
    let watch_profiles = profiles.0.signals().await.ok();
    let mut signals = futures_util::stream::select(
        futures_util::stream::iter(watch_upower).flatten(),
        futures_util::stream::iter(watch_profiles).flatten(),
    );

    let read = || async {
        let mut state = PowerState::default();
        if let Ok(p) = profiles.0.props(profiles.1, iface).await {
            state.active = p.str("ActiveProfile");
            state.degraded = p.str("PerformanceDegraded").filter(|d| !d.is_empty());
            state.profiles = profile_names(&p);
        }
        if let Ok(d) = upower.props(DISPLAY_DEVICE, DEVICE_IFACE).await {
            state.battery = battery(&d);
        }
        state
    };

    let mut state = read().await;
    emit(Event::State(state.clone()));
    let debounce = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(debounce);
    let mut dirty = false;
    loop {
        tokio::select! {
            req = requests.recv() => match req {
                Some(Request::SetProfile(profile)) => {
                    let (bus, path) = (profiles.0.clone(), profiles.1);
                    let emit = emit.clone();
                    tokio::spawn(async move {
                        if let Err(e) = bus.set(path, iface, "ActiveProfile", Value::from(profile.as_str())).await {
                            emit(Event::Error(format!("Couldn't change the power mode: {}", describe_error(&e))));
                        }
                    });
                }
                None => break,
            },
            Some(_) = signals.next() => {
                if !dirty {
                    debounce.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(150));
                    dirty = true;
                }
            }
            () = &mut debounce, if dirty => {
                dirty = false;
                let new = read().await;
                if new != state {
                    state = new;
                    emit(Event::State(state.clone()));
                }
            }
        }
    }
}

fn profile_names(p: &Props) -> Vec<String> {
    let Some(list) = p.0.get("Profiles").and_then(|v| v.try_clone().ok()) else { return Vec::new() };
    let Ok(list) = Vec::<HashMap<String, OwnedValue>>::try_from(list) else { return Vec::new() };
    let mut names: Vec<String> = list.into_iter().filter_map(|m| Props(m).str("Profile")).collect();
    // Lowest power first, as they're shown.
    let rank = |n: &str| ["power-saver", "balanced", "performance"].iter().position(|p| *p == n).unwrap_or(3);
    names.sort_by_key(|n| rank(n));
    names
}

fn battery(d: &Props) -> Option<Battery> {
    const BATTERY: u32 = 2;
    if d.u32("Type") != Some(BATTERY) || d.bool("IsPresent") != Some(true) {
        return None;
    }
    let charge = match d.u32("State") {
        Some(1) => Charge::Charging,
        Some(2) | Some(3) | Some(6) => Charge::Discharging,
        Some(4) => Charge::Full,
        _ => Charge::NotCharging,
    };
    let time_left = match charge {
        Charge::Charging => d.i64("TimeToFull"),
        Charge::Discharging => d.i64("TimeToEmpty"),
        _ => None,
    }
    .filter(|t| *t > 0)
    .map(|t| t as u64);
    Some(Battery { percent: d.f64("Percentage").unwrap_or(0.0), charge, time_left })
}
