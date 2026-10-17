//! Wi-Fi: on/off, the current network and its details, nearby and saved networks.

use std::cell::RefCell;
use std::rc::Rc;

use dmm_sys::network::{
    self, ActiveNetwork, Event, KnownNetwork, Network, Request, Security, WifiState,
};
use relm4::adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};
use tokio::sync::mpsc::UnboundedSender;

use crate::widgets::{Hero, KeyedList, KeyedRow, OffCard, confirm, tile};

pub struct WifiPage {
    tx: UnboundedSender<Request>,
    state: WifiState,
    scanning: bool,
    connecting: Option<String>,
    visible: bool,
}

#[derive(Debug)]
pub enum Input {
    /// The page was shown or hidden.
    Visible(bool),
    Toggle(bool),
    Scan,
    /// A network row was clicked.
    Activate(String),
    Disconnect,
    ConfirmForget(String),
    Forget(String),
    Password {
        ssid: String,
        password: String,
    },
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    off_group: adw::PreferencesGroup,
    off: OffCard,
    current_group: adw::PreferencesGroup,
    current: CurrentRow,
    available_group: adw::PreferencesGroup,
    scan_stack: gtk::Stack,
    available: KeyedList<NetworkRow>,
    known_group: adw::PreferencesGroup,
    known: KeyedList<KnownRow>,
}

impl Component for WifiPage {
    type CommandOutput = Event;
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
                .register(network::run(rx, move |event| {
                    let _ = out.send(event);
                }))
                .drop_on_shutdown()
        });

        let s = sender.clone();
        let hero = Hero::new("dmm-network-wireless-symbolic", "Wi-Fi", move |on| {
            s.input(Input::Toggle(on))
        });
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);

        let off = OffCard::new();
        let off_group = adw::PreferencesGroup::new();
        off_group.add(&off.root);

        let current = CurrentRow::new(&sender);
        let current_group = adw::PreferencesGroup::builder()
            .title("Current Network")
            .build();
        current_group.add(&current.list);

        let scan_button = gtk::Button::builder()
            .icon_name("dmm-view-refresh-symbolic")
            .tooltip_text("Scan for networks")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .build();
        let s = sender.clone();
        scan_button.connect_clicked(move |_| s.input(Input::Scan));
        let scan_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        scan_stack.add_named(&scan_button, Some("button"));
        scan_stack.add_named(&adw::Spinner::new(), Some("spinner"));

        let available = KeyedList::new(Some("No networks in range"));
        let available_group = adw::PreferencesGroup::builder()
            .title("Networks")
            .header_suffix(&scan_stack)
            .build();
        available_group.add(&available.list);

        let known = KeyedList::new(None);
        let known_group = adw::PreferencesGroup::builder()
            .title("Saved Networks")
            .description("Joined automatically when in range")
            .build();
        known_group.add(&known.list);

        root.add(&hero_group);
        root.add(&off_group);
        root.add(&current_group);
        root.add(&available_group);
        root.add(&known_group);

        let model = Self {
            tx,
            state: WifiState::default(),
            scanning: false,
            connecting: None,
            visible: false,
        };
        let mut widgets = Widgets {
            hero,
            off_group,
            off,
            current_group,
            current,
            available_group,
            scan_stack,
            available,
            known_group,
            known,
        };
        model.update_view(&mut widgets, sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(visible) => {
                self.visible = visible;
                if visible && self.state.enabled {
                    self.send(Request::Scan);
                }
            }
            Input::Toggle(on) => self.send(Request::SetEnabled(on)),
            Input::Scan => self.send(Request::Scan),
            Input::Activate(ssid) => {
                let Some(net) = self.state.networks.iter().find(|n| n.ssid == ssid) else {
                    return;
                };
                if net.active || self.connecting.as_deref() == Some(ssid.as_str()) {
                    return;
                }
                self.send(Request::Connect {
                    ssid,
                    password: None,
                });
            }
            Input::Disconnect => self.send(Request::Disconnect),
            Input::ConfirmForget(ssid) => {
                let s = sender.clone();
                let target = ssid.clone();
                confirm(
                    root,
                    &format!("Forget “{ssid}”?"),
                    "Its password is deleted and this computer stops joining it automatically.",
                    "Forget",
                    move || s.input(Input::Forget(target.clone())),
                );
            }
            Input::Forget(ssid) => self.send(Request::Forget(ssid)),
            Input::Password { ssid, password } => self.send(Request::Connect {
                ssid,
                password: Some(password),
            }),
        }
    }

    fn update_cmd(&mut self, event: Event, sender: ComponentSender<Self>, root: &Self::Root) {
        match event {
            Event::State(state) => {
                let turned_on = state.enabled && !self.state.enabled;
                self.state = state;
                if turned_on && self.visible {
                    self.send(Request::Scan);
                }
            }
            Event::Scanning(scanning) => self.scanning = scanning,
            Event::Connecting(ssid) => self.connecting = Some(ssid),
            Event::Connected(ssid) => {
                self.connecting = None;
                toast(&sender, format!("Connected to “{ssid}”"));
            }
            Event::PasswordNeeded {
                ssid,
                security,
                wrong,
            } => {
                self.connecting = None;
                password_dialog(root, &sender, ssid, security, wrong);
            }
            Event::Failed { ssid, message } => {
                self.connecting = None;
                toast(&sender, format!("Couldn't connect to “{ssid}”: {message}"));
            }
            Event::Error(message) => toast(&sender, message),
        }
    }

    fn update_view(&self, w: &mut Self::Widgets, sender: ComponentSender<Self>) {
        let st = &self.state;
        let usable = st.service && st.interface.is_some();
        let on = usable && st.enabled;

        w.hero.set_switch(st.enabled, usable && st.hardware_enabled);
        w.hero.set_on(on);
        w.hero.set_icon(match &st.active {
            _ if !on => "dmm-network-wireless-disabled-symbolic",
            Some(a) if a.connected => signal_icon(a.strength),
            Some(_) => "dmm-network-wireless-acquiring-symbolic",
            None => "dmm-network-wireless-offline-symbolic",
        });
        w.hero.set_subtitle(&match &st.active {
            _ if !st.service => "NetworkManager isn't running".to_string(),
            _ if st.interface.is_none() => "No Wi-Fi adapter".to_string(),
            _ if !st.hardware_enabled => "Turned off by a hardware switch".to_string(),
            _ if !st.enabled => "Off".to_string(),
            Some(a) if a.connected => format!("Connected to “{}”", a.ssid),
            Some(a) => format!("Connecting to “{}”…", a.ssid),
            None => match &self.connecting {
                Some(ssid) => format!("Connecting to “{ssid}”…"),
                None => "Not connected".to_string(),
            },
        });

        w.off_group.set_visible(!on);
        if !st.service {
            w.off.set(
                "dmm-network-wireless-offline-symbolic",
                "NetworkManager isn't running",
                "Wi-Fi settings need NetworkManager. Start it with “systemctl enable --now NetworkManager”.",
            );
        } else if st.interface.is_none() {
            w.off.set(
                "dmm-network-wireless-disabled-symbolic",
                "No Wi-Fi adapter",
                "No wireless device was found.",
            );
        } else {
            w.off.set(
                "dmm-network-wireless-disabled-symbolic",
                "Wi-Fi is off",
                "Turn it on to see networks nearby.",
            );
        }

        w.current_group.set_visible(on && st.active.is_some());
        if let Some(active) = &st.active {
            w.current.update(active);
        }

        w.available_group.set_visible(on);
        w.scan_stack
            .set_visible_child_name(if self.scanning { "spinner" } else { "button" });
        let networks = st
            .networks
            .iter()
            .filter(|n| !n.active)
            .map(|n| {
                let connecting = self.connecting.as_deref() == Some(n.ssid.as_str());
                (n.ssid.clone(), (n.clone(), connecting))
            })
            .collect();
        w.available
            .sync(networks, |ssid| NetworkRow::new(ssid, &sender));

        let known = st
            .known
            .iter()
            .map(|k| (format!("{}\n{}", k.ssid, k.name), k.clone()))
            .collect();
        w.known.sync(known, |_| KnownRow::new(&sender));
        w.known_group.set_visible(on && !w.known.is_empty());
    }
}

impl WifiPage {
    fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }
}

fn toast(sender: &ComponentSender<WifiPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn signal_icon(strength: u8) -> &'static str {
    match network::signal_level(strength) {
        0 => "dmm-network-wireless-signal-weak-symbolic",
        1 => "dmm-network-wireless-signal-ok-symbolic",
        2 => "dmm-network-wireless-signal-good-symbolic",
        _ => "dmm-network-wireless-signal-excellent-symbolic",
    }
}

/// The current network as an expander with its details.
struct CurrentRow {
    list: gtk::ListBox,
    row: adw::ExpanderRow,
    icon: gtk::Image,
    ssid: Rc<RefCell<String>>,
    ip: gtk::Label,
    gateway: gtk::Label,
    security: gtk::Label,
    band: gtk::Label,
    speed: gtk::Label,
}

impl CurrentRow {
    fn new(sender: &ComponentSender<WifiPage>) -> Self {
        let row = adw::ExpanderRow::builder().use_markup(false).build();
        let icon = tile("dmm-network-wireless-signal-excellent-symbolic");
        icon.add_css_class("accent");
        row.add_prefix(&icon);

        let disconnect = gtk::Button::builder()
            .label("Disconnect")
            .valign(gtk::Align::Center)
            .build();
        let s = sender.clone();
        disconnect.connect_clicked(move |_| s.input(Input::Disconnect));
        row.add_suffix(&disconnect);

        let detail = |title: &str| {
            let label = gtk::Label::builder()
                .selectable(true)
                .css_classes(["value-label"])
                .build();
            let r = adw::ActionRow::builder().title(title).build();
            r.add_suffix(&label);
            row.add_row(&r);
            label
        };
        let ip = detail("IP Address");
        let gateway = detail("Router");
        let security = detail("Security");
        let band = detail("Band");
        let speed = detail("Link Speed");

        let ssid = Rc::new(RefCell::new(String::new()));
        let forget = adw::ButtonRow::builder()
            .title("Forget This Network")
            .css_classes(["destructive-action"])
            .build();
        let (s, current) = (sender.clone(), ssid.clone());
        forget.connect_activated(move |_| s.input(Input::ConfirmForget(current.borrow().clone())));
        row.add_row(&forget);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        list.append(&row);
        Self {
            list,
            row,
            icon,
            ssid,
            ip,
            gateway,
            security,
            band,
            speed,
        }
    }

    fn update(&self, a: &ActiveNetwork) {
        self.ssid.replace(a.ssid.clone());
        self.row.set_title(&a.ssid);
        self.row.set_subtitle(if a.connected {
            "Connected"
        } else {
            "Connecting…"
        });
        self.icon.set_icon_name(Some(signal_icon(a.strength)));
        let dash = || "—".to_string();
        self.ip.set_label(&a.ipv4.clone().unwrap_or_else(dash));
        self.gateway
            .set_label(&a.gateway.clone().unwrap_or_else(dash));
        self.security.set_label(a.security.label());
        self.band.set_label(&format!(
            "{} · {} MHz",
            network::band(a.frequency),
            a.frequency
        ));
        self.speed.set_label(&if a.bitrate > 0 {
            format!("{} Mb/s", a.bitrate)
        } else {
            dash()
        });
    }
}

struct NetworkRow {
    row: adw::ActionRow,
    icon: gtk::Image,
    lock: gtk::Image,
    spinner: adw::Spinner,
}

impl NetworkRow {
    fn new(ssid: &str, sender: &ComponentSender<WifiPage>) -> Self {
        let row = adw::ActionRow::builder()
            .title(ssid)
            .use_markup(false)
            .activatable(true)
            .build();
        let icon = tile("dmm-network-wireless-signal-good-symbolic");
        row.add_prefix(&icon);
        let spinner = adw::Spinner::builder().visible(false).build();
        let lock = gtk::Image::builder()
            .icon_name("dmm-channel-secure-symbolic")
            .css_classes(["dim-label"])
            .build();
        row.add_suffix(&spinner);
        row.add_suffix(&lock);
        let (s, ssid) = (sender.clone(), ssid.to_string());
        row.connect_activated(move |_| s.input(Input::Activate(ssid.clone())));
        Self {
            row,
            icon,
            lock,
            spinner,
        }
    }
}

impl KeyedRow for NetworkRow {
    type Item = (Network, bool);

    fn widget(&self) -> &gtk::Widget {
        self.row.upcast_ref()
    }

    fn update(&self, (net, connecting): &(Network, bool)) {
        self.icon.set_icon_name(Some(signal_icon(net.strength)));
        let mut parts = Vec::new();
        if *connecting {
            parts.push("Connecting…".to_string());
        } else if net.known {
            parts.push("Saved".to_string());
        }
        parts.push(net.security.label().to_string());
        if net.security == Security::Enterprise && !net.known {
            parts.push("not supported yet".to_string());
        }
        parts.push(network::band(net.frequency).to_string());
        self.row.set_subtitle(&parts.join(" · "));
        self.lock.set_visible(!net.security.is_open());
        self.spinner.set_visible(*connecting);
    }
}

struct KnownRow {
    row: adw::ActionRow,
    ssid: Rc<RefCell<String>>,
}

impl KnownRow {
    fn new(sender: &ComponentSender<WifiPage>) -> Self {
        let row = adw::ActionRow::builder().use_markup(false).build();
        row.add_prefix(&tile("dmm-network-wireless-symbolic"));
        let forget = gtk::Button::builder()
            .icon_name("dmm-user-trash-symbolic")
            .tooltip_text("Forget")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .build();
        let ssid = Rc::new(RefCell::new(String::new()));
        let (s, current) = (sender.clone(), ssid.clone());
        forget.connect_clicked(move |_| s.input(Input::ConfirmForget(current.borrow().clone())));
        row.add_suffix(&forget);
        Self { row, ssid }
    }
}

impl KeyedRow for KnownRow {
    type Item = KnownNetwork;

    fn widget(&self) -> &gtk::Widget {
        self.row.upcast_ref()
    }

    fn update(&self, k: &KnownNetwork) {
        self.ssid.replace(k.ssid.clone());
        self.row.set_title(&k.ssid);
        self.row
            .set_subtitle(if k.name != k.ssid { &k.name } else { "" });
    }
}

fn password_dialog(
    parent: &adw::PreferencesPage,
    sender: &ComponentSender<WifiPage>,
    ssid: String,
    security: Security,
    wrong: bool,
) {
    let body = if wrong {
        "That password didn't work. Check it and try again."
    } else {
        "This network is protected by a password."
    };
    let dialog = adw::AlertDialog::new(Some(&format!("Join “{ssid}”")), Some(body));
    dialog.add_responses(&[("cancel", "Cancel"), ("connect", "Connect")]);
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("connect"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("connect", false);

    let entry = adw::PasswordEntryRow::builder().title("Password").build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(&entry);
    dialog.set_extra_child(Some(&list));

    let min = security.min_password_len();
    let d = dialog.clone();
    entry.connect_changed(move |e| {
        d.set_response_enabled("connect", e.text().chars().count() >= min)
    });

    let submit = {
        let (s, ssid) = (sender.clone(), ssid.clone());
        move |password: String| {
            s.input(Input::Password {
                ssid: ssid.clone(),
                password,
            })
        }
    };
    let (d, on_enter) = (dialog.clone(), submit.clone());
    entry.connect_entry_activated(move |e| {
        if e.text().chars().count() >= min {
            on_enter(e.text().to_string());
            d.force_close();
        }
    });
    let e = entry.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "connect" {
            submit(e.text().to_string());
        }
    });
    dialog.present(Some(parent));
    entry.grab_focus();
}
