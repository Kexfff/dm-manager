//! Wi-Fi through NetworkManager.
//!
//! [`run`] keeps a [`WifiState`] snapshot current (re-read after any NetworkManager signal,
//! debounced) and carries out [`Request`]s. Connecting reports its outcome as an [`Event`],
//! including when a password is needed or was wrong.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::join_all;
use tokio::sync::mpsc::UnboundedReceiver;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use crate::bus::{Bus, Emit, Props, describe_error};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION_IFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS_IFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const IP4_IFACE: &str = "org.freedesktop.NetworkManager.IP4Config";

const DEVICE_TYPE_WIFI: u32 = 2;
const DEVICE_PREPARE: u32 = 40;
const DEVICE_ACTIVATED: u32 = 100;
const DEVICE_FAILED: u32 = 120;

const DEBOUNCE: Duration = Duration::from_millis(150);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    /// Opportunistic Wireless Encryption: encrypted, but no password.
    Owe,
    Wep,
    Wpa,
    Wpa3,
    Enterprise,
}

impl Security {
    fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        const PRIVACY: u32 = 0x1;
        const PSK: u32 = 0x100;
        const EAP: u32 = 0x200;
        const SAE: u32 = 0x400;
        const OWE: u32 = 0x800;
        const EAP_SUITE_B: u32 = 0x2000;

        let key_mgmt = wpa | rsn;
        if key_mgmt & (EAP | EAP_SUITE_B) != 0 {
            Self::Enterprise
        } else if key_mgmt & PSK != 0 {
            // WPA2, or a WPA2/WPA3 transition network: NetworkManager picks the best.
            Self::Wpa
        } else if key_mgmt & SAE != 0 {
            Self::Wpa3
        } else if key_mgmt & OWE != 0 {
            Self::Owe
        } else if flags & PRIVACY != 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    pub fn needs_password(self) -> bool {
        matches!(self, Self::Wep | Self::Wpa | Self::Wpa3)
    }

    pub fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::Owe)
    }

    pub fn min_password_len(self) -> usize {
        if self == Self::Wep { 5 } else { 8 }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Owe => "Enhanced Open",
            Self::Wep => "WEP",
            Self::Wpa => "WPA2",
            Self::Wpa3 => "WPA3",
            Self::Enterprise => "Enterprise",
        }
    }
}

/// The band for a frequency in MHz.
pub fn band(frequency: u32) -> &'static str {
    match frequency {
        5925.. => "6 GHz",
        4900.. => "5 GHz",
        _ => "2.4 GHz",
    }
}

/// Signal strength (0–100) as 0–3 bars.
pub fn signal_level(strength: u8) -> u8 {
    match strength {
        0..=24 => 0,
        25..=49 => 1,
        50..=74 => 2,
        _ => 3,
    }
}

/// A visible network: the strongest access point for each SSID.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub strength: u8,
    pub security: Security,
    pub frequency: u32,
    /// A saved profile exists for this SSID.
    pub known: bool,
    pub active: bool,
    ap: String,
}

/// The network the device is connected (or connecting) to.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveNetwork {
    pub ssid: String,
    /// `false` while still connecting.
    pub connected: bool,
    pub strength: u8,
    pub security: Security,
    pub frequency: u32,
    /// Link speed in Mb/s.
    pub bitrate: u32,
    pub ipv4: Option<String>,
    pub gateway: Option<String>,
}

/// A saved Wi-Fi profile.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownNetwork {
    pub ssid: String,
    /// The profile's name (usually the SSID).
    pub name: String,
    path: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WifiState {
    /// NetworkManager is reachable.
    pub service: bool,
    /// The Wi-Fi interface, if there is one.
    pub interface: Option<String>,
    pub enabled: bool,
    /// `false` when a hardware switch (rfkill) turns Wi-Fi off.
    pub hardware_enabled: bool,
    pub active: Option<ActiveNetwork>,
    /// Sorted: active, then saved, then by signal, then by name.
    pub networks: Vec<Network>,
    pub known: Vec<KnownNetwork>,
    device: Option<String>,
}

#[derive(Debug)]
pub enum Request {
    SetEnabled(bool),
    Scan,
    /// Connect to a visible network. With a password, a saved profile's password is replaced.
    Connect {
        ssid: String,
        password: Option<String>,
    },
    Disconnect,
    /// Delete every saved profile for this SSID.
    Forget(String),
}

#[derive(Debug)]
pub enum Event {
    State(WifiState),
    Scanning(bool),
    Connecting(String),
    Connected(String),
    /// Ask for a password and send `Connect` again with it.
    PasswordNeeded {
        ssid: String,
        security: Security,
        wrong: bool,
    },
    Failed {
        ssid: String,
        message: String,
    },
    Error(String),
}

pub async fn run(mut requests: UnboundedReceiver<Request>, emit: impl Fn(Event) + Send + Sync + 'static) {
    let emit: Emit<Event> = Arc::new(emit);
    let conn = match Connection::system().await {
        Ok(conn) => conn,
        Err(e) => {
            emit(Event::State(WifiState::default()));
            emit(Event::Error(format!("No system bus: {}", describe_error(&e))));
            while requests.recv().await.is_some() {}
            return;
        }
    };
    let nm = Nm { bus: Bus::new(conn, SERVICE), emit: emit.clone() };

    let mut signals = match nm.bus.signals().await {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("can't watch NetworkManager: {e}");
            None
        }
    };
    let mut known = nm.known().await;
    let mut state = nm.snapshot(&known).await;
    emit(Event::State(state.clone()));

    let debounce = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(debounce);
    let (mut dirty, mut known_dirty) = (false, false);

    loop {
        tokio::select! {
            req = requests.recv() => match req {
                Some(req) => nm.handle(req, &state),
                None => break,
            },
            msg = next_signal(&mut signals) => match msg {
                Some(Ok(msg)) => {
                    let header = msg.header();
                    let path = header.path().map(|p| p.as_str()).unwrap_or_default();
                    let interface = header.interface().map(|i| i.as_str()).unwrap_or_default();
                    // Profiles only change through Settings (or NetworkManager restarting).
                    if path.starts_with(SETTINGS_PATH) || interface == "org.freedesktop.DBus" {
                        known_dirty = true;
                    }
                    if !dirty {
                        debounce.as_mut().reset(tokio::time::Instant::now() + DEBOUNCE);
                        dirty = true;
                    }
                }
                Some(Err(e)) => log::debug!("NetworkManager signal: {e}"),
                None => signals = None,
            },
            () = &mut debounce, if dirty => {
                dirty = false;
                if std::mem::take(&mut known_dirty) {
                    known = nm.known().await;
                }
                let new = nm.snapshot(&known).await;
                if new != state {
                    state = new;
                    emit(Event::State(state.clone()));
                }
            }
        }
    }
}

type Signals = futures_util::stream::Select<MessageStream, MessageStream>;

async fn next_signal(signals: &mut Option<Signals>) -> Option<zbus::Result<zbus::Message>> {
    match signals {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

struct Nm {
    bus: Bus,
    emit: Emit<Event>,
}

/// Everything a connection attempt needs, copied out of the current state.
struct Target {
    ssid: String,
    security: Security,
    ap: String,
    device: String,
    profiles: Vec<String>,
}

enum Outcome {
    Connected,
    NeedPassword { wrong: bool },
}

impl Nm {
    fn handle(&self, req: Request, state: &WifiState) {
        let bus = self.bus.clone();
        let emit = self.emit.clone();
        let device = state.device.clone();
        match req {
            Request::SetEnabled(on) => {
                tokio::spawn(async move {
                    if let Err(e) = bus.set(NM_PATH, NM_IFACE, "WirelessEnabled", Value::from(on)).await {
                        emit(Event::Error(format!(
                            "Couldn't turn Wi-Fi {}: {}",
                            if on { "on" } else { "off" },
                            describe_error(&e)
                        )));
                    }
                });
            }
            Request::Scan => {
                if let Some(device) = device {
                    tokio::spawn(scan(bus, emit, device));
                }
            }
            Request::Connect { ssid, password } => {
                let Some(device) = device else { return };
                let Some(net) = state.networks.iter().find(|n| n.ssid == ssid) else {
                    emit(Event::Failed { ssid, message: "The network is out of range".into() });
                    return;
                };
                let target = Target {
                    security: net.security,
                    ap: net.ap.clone(),
                    device,
                    profiles: profiles_for(state, &ssid),
                    ssid,
                };
                tokio::spawn(connect(bus, emit, target, password));
            }
            Request::Disconnect => {
                let Some(device) = device else { return };
                tokio::spawn(async move {
                    if let Err(e) = bus.invoke(&device, DEVICE_IFACE, "Disconnect", &()).await {
                        emit(Event::Error(format!("Couldn't disconnect: {}", describe_error(&e))));
                    }
                });
            }
            Request::Forget(ssid) => {
                let profiles = profiles_for(state, &ssid);
                tokio::spawn(async move {
                    for path in profiles {
                        if let Err(e) = bus.invoke(&path, CONNECTION_IFACE, "Delete", &()).await {
                            emit(Event::Error(format!("Couldn't forget “{ssid}”: {}", describe_error(&e))));
                        }
                    }
                });
            }
        }
    }

    async fn known(&self) -> Vec<KnownNetwork> {
        let Ok(paths) =
            self.bus.call::<_, Vec<OwnedObjectPath>>(SETTINGS_PATH, SETTINGS_IFACE, "ListConnections", &()).await
        else {
            return Vec::new();
        };
        let settings = join_all(paths.iter().map(|p| {
            self.bus.call::<_, HashMap<String, HashMap<String, OwnedValue>>>(
                p.as_str(),
                CONNECTION_IFACE,
                "GetSettings",
                &(),
            )
        }))
        .await;

        let mut known = Vec::new();
        for (path, settings) in paths.iter().zip(settings) {
            let Ok(mut settings) = settings else { continue };
            let connection = Props(settings.remove("connection").unwrap_or_default());
            if connection.str("type").as_deref() != Some("802-11-wireless") {
                continue;
            }
            let wifi = Props(settings.remove("802-11-wireless").unwrap_or_default());
            // Hotspots this machine creates aren't networks to join.
            if wifi.str("mode").as_deref() == Some("ap") {
                continue;
            }
            let ssid = String::from_utf8_lossy(&wifi.bytes("ssid").unwrap_or_default()).into_owned();
            if ssid.is_empty() {
                continue;
            }
            known.push(KnownNetwork {
                name: connection.str("id").unwrap_or_else(|| ssid.clone()),
                ssid,
                path: path.to_string(),
            });
        }
        known.sort_by_key(|k| k.ssid.to_lowercase());
        known
    }

    async fn snapshot(&self, known: &[KnownNetwork]) -> WifiState {
        self.try_snapshot(known).await.unwrap_or_else(|e| {
            log::debug!("NetworkManager unavailable: {e}");
            WifiState::default()
        })
    }

    async fn try_snapshot(&self, known: &[KnownNetwork]) -> zbus::Result<WifiState> {
        let nm = self.bus.props(NM_PATH, NM_IFACE).await?;
        let mut state = WifiState {
            service: true,
            enabled: nm.bool("WirelessEnabled").unwrap_or(false),
            hardware_enabled: nm.bool("WirelessHardwareEnabled").unwrap_or(true),
            known: known.to_vec(),
            ..Default::default()
        };

        let devices: Vec<OwnedObjectPath> = self.bus.call(NM_PATH, NM_IFACE, "GetDevices", &()).await?;
        let mut wifi = None;
        for path in devices {
            let props = self.bus.props(path.as_str(), DEVICE_IFACE).await?;
            if props.u32("DeviceType") == Some(DEVICE_TYPE_WIFI) {
                wifi = Some((path.to_string(), props));
                break;
            }
        }
        let Some((device, dev)) = wifi else {
            return Ok(state);
        };
        state.interface = dev.str("Interface");
        let wireless = self.bus.props(&device, WIRELESS_IFACE).await?;
        let active_ap = wireless.str("ActiveAccessPoint").filter(|p| p != "/");
        let device_state = dev.u32("State").unwrap_or(0);

        let aps: Vec<OwnedObjectPath> =
            self.bus.call(&device, WIRELESS_IFACE, "GetAllAccessPoints", &()).await.unwrap_or_default();
        let props = join_all(aps.iter().map(|ap| self.bus.props(ap.as_str(), AP_IFACE))).await;
        let known_ssids: HashSet<&str> = known.iter().map(|k| k.ssid.as_str()).collect();

        let mut by_ssid: HashMap<String, Network> = HashMap::new();
        for (ap, props) in aps.iter().zip(props) {
            let Ok(p) = props else { continue };
            let ssid = String::from_utf8_lossy(&p.bytes("Ssid").unwrap_or_default()).into_owned();
            if ssid.is_empty() {
                continue;
            }
            let net = Network {
                known: known_ssids.contains(ssid.as_str()),
                active: active_ap.as_deref() == Some(ap.as_str()),
                strength: p.u8("Strength").unwrap_or(0),
                security: Security::from_flags(
                    p.u32("Flags").unwrap_or(0),
                    p.u32("WpaFlags").unwrap_or(0),
                    p.u32("RsnFlags").unwrap_or(0),
                ),
                frequency: p.u32("Frequency").unwrap_or(0),
                ap: ap.to_string(),
                ssid,
            };
            match by_ssid.get_mut(&net.ssid) {
                // The active access point represents its SSID; otherwise the strongest does.
                Some(cur) if cur.active || (!net.active && cur.strength >= net.strength) => {}
                _ => {
                    by_ssid.insert(net.ssid.clone(), net);
                }
            }
        }

        if (DEVICE_PREPARE..=DEVICE_ACTIVATED).contains(&device_state)
            && let Some(net) = by_ssid.values().find(|n| n.active)
        {
            let mut active = ActiveNetwork {
                ssid: net.ssid.clone(),
                connected: device_state == DEVICE_ACTIVATED,
                strength: net.strength,
                security: net.security,
                frequency: net.frequency,
                bitrate: wireless.u32("Bitrate").unwrap_or(0) / 1000,
                ipv4: None,
                gateway: None,
            };
            if let Some(ip4) = dev.str("Ip4Config").filter(|p| p != "/")
                && let Ok(ip) = self.bus.props(&ip4, IP4_IFACE).await
            {
                active.gateway = ip.str("Gateway").filter(|g| !g.is_empty());
                active.ipv4 = first_address(&ip);
            }
            state.active = Some(active);
        }

        let mut networks: Vec<Network> = by_ssid.into_values().collect();
        networks.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then(b.known.cmp(&a.known))
                .then(signal_level(b.strength).cmp(&signal_level(a.strength)))
                .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
        });
        state.networks = networks;
        state.device = Some(device);
        Ok(state)
    }
}

fn profiles_for(state: &WifiState, ssid: &str) -> Vec<String> {
    state.known.iter().filter(|k| k.ssid == ssid).map(|k| k.path.clone()).collect()
}

/// `address/prefix` of the first IPv4 address in an IP4Config's `AddressData`.
fn first_address(ip: &Props) -> Option<String> {
    let data = ip.0.get("AddressData")?.try_clone().ok()?;
    let list = Vec::<HashMap<String, OwnedValue>>::try_from(data).ok()?;
    let first = Props(list.into_iter().next()?);
    let address = first.str("address")?;
    Some(match first.u32("prefix") {
        Some(prefix) => format!("{address}/{prefix}"),
        None => address,
    })
}

async fn scan(bus: Bus, emit: Emit<Event>, device: String) {
    emit(Event::Scanning(true));
    let before = last_scan(&bus, &device).await;
    match bus.invoke(&device, WIRELESS_IFACE, "RequestScan", &(HashMap::<&str, Value>::new(),)).await {
        // A scan takes a few seconds; LastScan moves when it's done.
        Ok(()) => {
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if last_scan(&bus, &device).await != before {
                    break;
                }
            }
        }
        // Usually "scanning not allowed" right after another scan; nothing to report.
        Err(e) => log::debug!("scan: {e}"),
    }
    emit(Event::Scanning(false));
}

async fn last_scan(bus: &Bus, device: &str) -> i64 {
    match bus.get(device, WIRELESS_IFACE, "LastScan").await.as_deref() {
        Ok(Value::I64(n)) => *n,
        _ => 0,
    }
}

async fn connect(bus: Bus, emit: Emit<Event>, target: Target, password: Option<String>) {
    let ssid = target.ssid.clone();
    emit(Event::Connecting(ssid.clone()));
    match try_connect(&bus, &target, password.as_deref()).await {
        Ok(Outcome::Connected) => emit(Event::Connected(ssid)),
        Ok(Outcome::NeedPassword { wrong }) => emit(Event::PasswordNeeded { ssid, security: target.security, wrong }),
        Err(message) => emit(Event::Failed { ssid, message }),
    }
}

async fn try_connect(bus: &Bus, t: &Target, password: Option<&str>) -> Result<Outcome, String> {
    let err = |e: zbus::Error| describe_error(&e);
    if t.security == Security::Enterprise && t.profiles.is_empty() {
        return Err("Enterprise (802.1X) networks can't be set up here yet; use nmcli or nm-connection-editor".into());
    }
    if t.profiles.is_empty() && password.is_none() && t.security.needs_password() {
        return Ok(Outcome::NeedPassword { wrong: false });
    }

    // Subscribe before activating so no state change is missed.
    let mut states = device_states(bus, &t.device).await.map_err(err)?;
    let device = ObjectPath::try_from(t.device.as_str()).map_err(|e| e.to_string())?;
    let ap = ObjectPath::try_from(t.ap.as_str()).map_err(|e| e.to_string())?;

    let mut created = None;
    match (password, t.profiles.first()) {
        (Some(pw), Some(profile)) => {
            update_password(bus, profile, t.security, pw).await.map_err(err)?;
            activate(bus, profile, &device, &ap).await.map_err(err)?;
        }
        (None, Some(profile)) => activate(bus, profile, &device, &ap).await.map_err(err)?,
        (pw, None) => {
            let mut settings: HashMap<&str, HashMap<&str, Value>> = HashMap::new();
            if let Some(pw) = pw {
                settings.insert("802-11-wireless-security", security_settings(t.security, pw));
            }
            // NetworkManager fills in the rest (SSID, mode) from the access point.
            let (profile, _active): (OwnedObjectPath, OwnedObjectPath) = bus
                .call(NM_PATH, NM_IFACE, "AddAndActivateConnection", &(settings, &device, &ap))
                .await
                .map_err(err)?;
            created = Some(profile);
        }
    }

    let result = tokio::time::timeout(CONNECT_TIMEOUT, async {
        while let Some(msg) = states.next().await {
            let Ok(msg) = msg else { continue };
            let Ok((new, _old, reason)) = msg.body().deserialize::<(u32, u32, u32)>() else {
                continue;
            };
            match new {
                DEVICE_ACTIVATED => return Ok(()),
                DEVICE_FAILED => return Err(reason),
                _ => {}
            }
        }
        Err(0)
    })
    .await;

    let failure = match result {
        Ok(Ok(())) => return Ok(Outcome::Connected),
        Ok(Err(reason)) => reason,
        Err(_) => return Err("Timed out".into()),
    };
    // Don't keep a new profile that never worked (e.g. with a mistyped password).
    if let Some(profile) = created {
        let _ = bus.invoke(profile.as_str(), CONNECTION_IFACE, "Delete", &()).await;
    }
    if t.security.needs_password() && is_secrets_failure(failure) {
        Ok(Outcome::NeedPassword { wrong: true })
    } else {
        Err(failure_text(failure))
    }
}

async fn activate(bus: &Bus, profile: &str, device: &ObjectPath<'_>, ap: &ObjectPath<'_>) -> zbus::Result<()> {
    let profile = ObjectPath::try_from(profile)?;
    bus.call::<_, OwnedObjectPath>(NM_PATH, NM_IFACE, "ActivateConnection", &(profile, device, ap)).await.map(drop)
}

async fn device_states(bus: &Bus, device: &str) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(SERVICE)?
        .path(device)?
        .interface(DEVICE_IFACE)?
        .member("StateChanged")?
        .build();
    MessageStream::for_match_rule(rule, &bus.conn, Some(64)).await
}

fn security_settings(security: Security, password: &str) -> HashMap<&'static str, Value<'static>> {
    let mut s = HashMap::new();
    match security {
        Security::Wep => {
            // 5/13 characters or 10/26 hex digits are raw keys; anything else is a passphrase.
            let raw = matches!(password.len(), 5 | 13)
                || (matches!(password.len(), 10 | 26) && password.chars().all(|c| c.is_ascii_hexdigit()));
            s.insert("key-mgmt", Value::from("none"));
            s.insert("wep-key0", Value::from(password.to_owned()));
            s.insert("wep-key-type", Value::from(if raw { 1u32 } else { 2u32 }));
        }
        Security::Wpa3 => {
            s.insert("key-mgmt", Value::from("sae"));
            s.insert("psk", Value::from(password.to_owned()));
        }
        _ => {
            s.insert("key-mgmt", Value::from("wpa-psk"));
            s.insert("psk", Value::from(password.to_owned()));
        }
    }
    s
}

/// Replaces a saved profile's password, keeping its other settings.
async fn update_password(bus: &Bus, profile: &str, security: Security, password: &str) -> zbus::Result<()> {
    let mut settings: HashMap<String, HashMap<String, OwnedValue>> =
        bus.call(profile, CONNECTION_IFACE, "GetSettings", &()).await?;
    // These deprecated forms duplicate address-data/route-data; sending both is ambiguous.
    for ip in ["ipv4", "ipv6"] {
        if let Some(section) = settings.get_mut(ip) {
            section.remove("addresses");
            section.remove("routes");
        }
    }
    let section = settings.entry("802-11-wireless-security".into()).or_default();
    for (key, value) in security_settings(security, password) {
        section.insert(key.into(), OwnedValue::try_from(value)?);
    }
    bus.invoke(profile, CONNECTION_IFACE, "Update", &(settings,)).await
}

/// Device state reasons that mean the password was refused.
fn is_secrets_failure(reason: u32) -> bool {
    // NO_SECRETS, SUPPLICANT_DISCONNECT, SUPPLICANT_TIMEOUT
    matches!(reason, 7 | 8 | 11)
}

fn failure_text(reason: u32) -> String {
    match reason {
        5 | 15..=17 => "Couldn't get an IP address".into(),
        9 | 10 => "Wi-Fi authentication failed".into(),
        53 => "The network is out of range".into(),
        n => format!("Couldn't connect (reason {n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_from_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(1, 0, 0), Security::Wep);
        assert_eq!(Security::from_flags(1, 0, 0x188), Security::Wpa);
        assert_eq!(Security::from_flags(1, 0, 0x588), Security::Wpa); // WPA2/WPA3 transition
        assert_eq!(Security::from_flags(1, 0, 0x488), Security::Wpa3);
        assert_eq!(Security::from_flags(1, 0, 0x288), Security::Enterprise);
        assert_eq!(Security::from_flags(1, 0, 0x888), Security::Owe);
    }

    #[test]
    fn bands() {
        assert_eq!(band(2437), "2.4 GHz");
        assert_eq!(band(5180), "5 GHz");
        assert_eq!(band(5955), "6 GHz");
    }
}
