//! Power & Sleep: power mode, battery, and when the screen turns off, locks and the computer
//! sleeps (swayidle).

use dmm_sys::idle::{self, IdleConfig, Timer};
use dmm_sys::power::{self, Charge, Event, PowerState, Request};
use relm4::adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};
use tokio::sync::mpsc::UnboundedSender;

use crate::widgets::{Bindings, Hero, combo_row, switch_row};

const SCREEN_OFF: &[u32] = &[60, 120, 180, 300, 600, 900, 1800];
const LOCK: &[u32] = &[60, 120, 300, 600, 900, 1800, 3600];
const SUSPEND: &[u32] = &[300, 600, 900, 1200, 1800, 2700, 3600, 7200];

pub struct PowerPage {
    tx: UnboundedSender<Request>,
    state: PowerState,
    idle: IdleConfig,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Profile(String),
    Timer(Timer, Option<u32>),
    LockBeforeSleep(bool),
}

#[derive(Debug)]
pub enum Cmd {
    Power(Event),
    Restarted(Result<(), String>),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    mode_group: adw::PreferencesGroup,
    modes: adw::ToggleGroup,
    idle_group: adw::PreferencesGroup,
    bindings: Bindings<IdleConfig>,
}

impl Component for PowerPage {
    type CommandOutput = Cmd;
    type Input = Input;
    type Output = Output;
    type Init = ();
    type Root = adw::PreferencesPage;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        adw::PreferencesPage::new()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        sender.command(move |out, shutdown| {
            shutdown
                .register(power::run(rx, move |event| {
                    let _ = out.send(Cmd::Power(event));
                }))
                .drop_on_shutdown()
        });

        let hero = Hero::new("dmm-ac-adapter-symbolic", "Power & Sleep");
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);
        root.add(&hero_group);

        let modes = adw::ToggleGroup::builder().homogeneous(true).css_classes(["power-modes"]).build();
        for (name, label, icon) in [
            ("power-saver", "Power Saver", "dmm-power-profile-power-saver-symbolic"),
            ("balanced", "Balanced", "dmm-power-profile-balanced-symbolic"),
            ("performance", "Performance", "dmm-power-profile-performance-symbolic"),
        ] {
            let content = gtk::Box::builder().spacing(8).halign(gtk::Align::Center).build();
            content.append(&gtk::Image::from_icon_name(icon));
            content.append(&gtk::Label::new(Some(label)));
            modes.add(adw::Toggle::builder().name(name).child(&content).build());
        }
        let s = sender.clone();
        modes.connect_active_name_notify(move |g| {
            if let Some(name) = g.active_name() {
                s.input(Input::Profile(name.to_string()));
            }
        });
        let mode_group = adw::PreferencesGroup::builder().title("Power Mode").build();
        mode_group.add(&modes);
        root.add(&mode_group);

        let mut bindings = Bindings::new();
        let idle_group = adw::PreferencesGroup::builder()
            .title("When Idle")
            .description("Apps playing video, or dmsh's Keep awake, pause these timers")
            .build();
        idle_group.add(&timer_row("Turn Screen Off", Timer::ScreenOff, SCREEN_OFF, &sender, &mut bindings));
        idle_group.add(&timer_row("Lock Screen", Timer::Lock, LOCK, &sender, &mut bindings));
        idle_group.add(&timer_row("Sleep", Timer::Suspend, SUSPEND, &sender, &mut bindings));
        let lock = switch_row("Lock Before Sleeping", "So the computer wakes up locked");
        let (s, quiet) = (sender.clone(), bindings.quiet());
        lock.connect_active_notify(move |r| {
            if !quiet.get() {
                s.input(Input::LockBeforeSleep(r.is_active()));
            }
        });
        let l = lock.clone();
        bindings.add(move |c: &IdleConfig| l.set_active(c.lock_before_sleep()));
        idle_group.add(&lock);
        root.add(&idle_group);

        let model = Self { tx, state: PowerState::default(), idle: IdleConfig::load(&IdleConfig::default_path()) };
        let mut widgets = Widgets { hero, mode_group, modes, idle_group, bindings };
        model.show(&mut widgets);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, _: &Self::Root) {
        match input {
            Input::Visible(true) => self.idle = IdleConfig::load(&IdleConfig::default_path()),
            Input::Visible(false) => return,
            Input::Profile(profile) => {
                if self.state.active.as_deref() != Some(profile.as_str()) {
                    let _ = self.tx.send(Request::SetProfile(profile));
                }
                return;
            }
            Input::Timer(timer, seconds) => {
                self.idle.set_timeout(timer, seconds);
                self.save_idle(&sender);
            }
            Input::LockBeforeSleep(on) => {
                self.idle.set_lock_before_sleep(on);
                self.save_idle(&sender);
            }
        }
        self.show(w);
    }

    fn update_cmd_with_view(&mut self, w: &mut Widgets, cmd: Cmd, sender: ComponentSender<Self>, _: &Self::Root) {
        match cmd {
            Cmd::Power(Event::State(state)) => self.state = state,
            Cmd::Power(Event::Error(e)) => toast(&sender, e),
            Cmd::Restarted(Err(e)) => toast(&sender, format!("Saved, but swayidle didn't restart: {e}")),
            Cmd::Restarted(Ok(())) => {}
        }
        self.show(w);
    }
}

impl PowerPage {
    fn save_idle(&self, sender: &ComponentSender<Self>) {
        if let Err(e) = self.idle.save() {
            toast(sender, e);
            return;
        }
        // swayidle reads its config only at start.
        if idle::is_running() {
            sender.oneshot_command(async {
                Cmd::Restarted(tokio::task::spawn_blocking(idle::restart).await.unwrap_or_else(|e| Err(e.to_string())))
            });
        }
    }

    fn show(&self, w: &mut Widgets) {
        let st = &self.state;
        let (icon, subtitle) = match &st.battery {
            None => ("dmm-ac-adapter-symbolic", "On mains power".to_string()),
            Some(b) => {
                let percent = b.percent.round();
                let time = b.time_left.map(|t| format!(" · {}", duration(t))).unwrap_or_default();
                match b.charge {
                    Charge::Charging => {
                        ("dmm-battery-level-80-charging-symbolic", format!("Charging · {percent}%{time} until full"))
                    }
                    Charge::Discharging => ("dmm-battery-level-80-symbolic", format!("Battery {percent}%{time} left")),
                    Charge::Full => ("dmm-battery-full-charged-symbolic", "Fully charged".to_string()),
                    Charge::NotCharging => {
                        ("dmm-battery-level-80-symbolic", format!("Plugged in · {percent}%, not charging"))
                    }
                }
            }
        };
        w.hero.set_icon(icon);
        w.hero.set_subtitle(&subtitle);

        w.mode_group.set_visible(!st.profiles.is_empty());
        if let Some(active) = &st.active
            && w.modes.active_name().as_deref() != Some(active.as_str())
        {
            w.modes.set_active_name(Some(active));
        }
        w.mode_group.set_description(
            st.degraded.as_ref().map(|d| format!("Performance is held back: {}", d.replace('-', " "))).as_deref(),
        );

        if !self.idle.exists() {
            w.idle_group.set_description(Some(
                "There's no swayidle config yet; changing these creates ~/.config/swayidle/config. swayidle must be started (e.g. by niri) to use it.",
            ));
        }
        w.bindings.refresh(&self.idle);
    }
}

fn toast(sender: &ComponentSender<PowerPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

/// "1 h 20 min", "45 min"
fn duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// "5 minutes", "1 hour", "Never"
fn label(seconds: Option<u32>) -> String {
    match seconds {
        None => "Never".into(),
        Some(s) if s % 3600 == 0 => match s / 3600 {
            1 => "1 hour".into(),
            h => format!("{h} hours"),
        },
        Some(s) if s % 60 == 0 => match s / 60 {
            1 => "1 minute".into(),
            m => format!("{m} minutes"),
        },
        Some(s) => format!("{s} seconds"),
    }
}

/// A choice of times for a timer; a time set by hand that isn't offered is added to the list.
fn timer_row(
    title: &str,
    timer: Timer,
    offered: &'static [u32],
    sender: &ComponentSender<PowerPage>,
    bindings: &mut Bindings<IdleConfig>,
) -> adw::ComboRow {
    let row = combo_row(title, "", &[]);
    let choices: std::rc::Rc<std::cell::RefCell<Vec<Option<u32>>>> = std::rc::Rc::default();
    let (s, quiet, ch) = (sender.clone(), bindings.quiet(), choices.clone());
    row.connect_selected_notify(move |r| {
        if quiet.get() {
            return;
        }
        if let Some(choice) = ch.borrow().get(r.selected() as usize) {
            s.input(Input::Timer(timer, *choice));
        }
    });
    let r = row.clone();
    bindings.add(move |c| {
        let current = c.timeout(timer);
        let mut list: Vec<Option<u32>> = std::iter::once(None).chain(offered.iter().copied().map(Some)).collect();
        if !list.contains(&current) {
            list.push(current);
            list[1..].sort();
        }
        let labels: Vec<String> = list.iter().map(|s| label(*s)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        r.set_model(Some(&gtk::StringList::new(&labels)));
        r.set_selected(list.iter().position(|s| *s == current).unwrap_or(0) as u32);
        *choices.borrow_mut() = list;
    });
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label(None), "Never");
        assert_eq!(label(Some(60)), "1 minute");
        assert_eq!(label(Some(300)), "5 minutes");
        assert_eq!(label(Some(7200)), "2 hours");
        assert_eq!(label(Some(90)), "90 seconds");
        assert_eq!(duration(4800), "1 h 20 min");
        assert_eq!(duration(2700), "45 min");
    }
}
