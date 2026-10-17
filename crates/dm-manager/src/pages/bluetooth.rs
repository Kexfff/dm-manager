//! Bluetooth: on/off, paired devices, nearby devices and pairing prompts.

use std::collections::HashSet;

use dmm_sys::bluetooth::{self, BluetoothState, Device, Event, Prompt, Reply, Request};
use relm4::adw::prelude::*;
use relm4::gtk::gio;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};
use tokio::sync::mpsc::UnboundedSender;

use crate::widgets::{Hero, KeyedList, KeyedRow, OffCard, confirm, tile};

pub struct BluetoothPage {
    tx: UnboundedSender<Request>,
    state: BluetoothState,
    busy: HashSet<String>,
    visible: bool,
    prompt: Option<adw::AlertDialog>,
}

#[derive(Debug)]
pub enum Input {
    /// The page was shown or hidden; nearby devices are only searched for while it's shown.
    Visible(bool),
    Toggle(bool),
    /// A device row was clicked: pair, connect or disconnect.
    Activate(String),
    ConfirmForget(String),
    Forget(String),
    Reply {
        id: u32,
        reply: Reply,
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
    paired_group: adw::PreferencesGroup,
    paired: KeyedList<DeviceRow>,
    nearby_group: adw::PreferencesGroup,
    searching: adw::Spinner,
    nearby: KeyedList<DeviceRow>,
}

impl Component for BluetoothPage {
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
                .register(bluetooth::run(rx, move |event| {
                    let _ = out.send(event);
                }))
                .drop_on_shutdown()
        });

        let s = sender.clone();
        let hero = Hero::new("dmm-bluetooth-active-symbolic", "Bluetooth", move |on| {
            s.input(Input::Toggle(on))
        });
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);

        let off = OffCard::new();
        let off_group = adw::PreferencesGroup::new();
        off_group.add(&off.root);

        let paired = KeyedList::new(None);
        let paired_group = adw::PreferencesGroup::builder().title("My Devices").build();
        paired_group.add(&paired.list);

        let searching = adw::Spinner::builder().valign(gtk::Align::Center).build();
        let nearby = KeyedList::new(Some(
            "Looking for devices… Make sure yours is in pairing mode.",
        ));
        let nearby_group = adw::PreferencesGroup::builder()
            .title("Nearby Devices")
            .header_suffix(&searching)
            .build();
        nearby_group.add(&nearby.list);

        root.add(&hero_group);
        root.add(&off_group);
        root.add(&paired_group);
        root.add(&nearby_group);

        let model = Self {
            tx,
            state: BluetoothState::default(),
            busy: HashSet::new(),
            visible: false,
            prompt: None,
        };
        let mut widgets = Widgets {
            hero,
            off_group,
            off,
            paired_group,
            paired,
            nearby_group,
            searching,
            nearby,
        };
        model.update_view(&mut widgets, sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(visible) => {
                self.visible = visible;
                if self.powered() {
                    self.send(Request::Discover(visible));
                }
            }
            Input::Toggle(on) => self.send(Request::SetPowered(on)),
            Input::Activate(path) => {
                if self.busy.contains(&path) {
                    return;
                }
                let Some(device) = self.state.devices.iter().find(|d| d.path == path) else {
                    return;
                };
                self.send(match (device.paired, device.connected) {
                    (false, _) => Request::Pair(path),
                    (true, true) => Request::Disconnect(path),
                    (true, false) => Request::Connect(path),
                });
            }
            Input::ConfirmForget(path) => {
                let Some(device) = self.state.devices.iter().find(|d| d.path == path) else {
                    return;
                };
                let s = sender.clone();
                confirm(
                    root,
                    &format!("Forget {}?", device.name),
                    "It's unpaired and has to be paired again to connect.",
                    "Forget",
                    move || s.input(Input::Forget(path.clone())),
                );
            }
            Input::Forget(path) => self.send(Request::Remove(path)),
            Input::Reply { id, reply } => self.send(Request::Reply { id, reply }),
        }
    }

    fn update_cmd(&mut self, event: Event, sender: ComponentSender<Self>, root: &Self::Root) {
        match event {
            Event::State(state) => {
                let was_powered = self.powered();
                self.state = state;
                // Discovery stops when the adapter powers off; resume it.
                if self.visible && self.powered() && !was_powered {
                    self.send(Request::Discover(true));
                }
            }
            Event::Busy { path, busy } => {
                if busy {
                    self.busy.insert(path);
                } else {
                    self.busy.remove(&path);
                }
            }
            Event::Error(message) => {
                let _ = sender.output(Output::Toast(message));
            }
            Event::Prompt { id, device, prompt } => {
                if let Some(old) = self.prompt.take() {
                    old.force_close();
                }
                self.prompt = Some(prompt_dialog(root, &sender, id, &device, prompt));
            }
            Event::PromptCanceled => {
                if let Some(dialog) = self.prompt.take() {
                    dialog.force_close();
                }
            }
        }
    }

    fn update_view(&self, w: &mut Self::Widgets, sender: ComponentSender<Self>) {
        let st = &self.state;
        let adapter = st.adapter.as_ref();
        let on = self.powered();

        w.hero.set_switch(on, adapter.is_some());
        w.hero.set_on(on);
        w.hero.set_icon(if on {
            "dmm-bluetooth-active-symbolic"
        } else {
            "dmm-bluetooth-disabled-symbolic"
        });
        let connected: Vec<&str> = st
            .devices
            .iter()
            .filter(|d| d.connected)
            .map(|d| d.name.as_str())
            .collect();
        w.hero.set_subtitle(&match (adapter, connected.as_slice()) {
            _ if !st.service => "The Bluetooth service isn't running".to_string(),
            (None, _) => "No Bluetooth adapter".to_string(),
            _ if !on => "Off".to_string(),
            (_, []) => "On · no devices connected".to_string(),
            (_, [one]) => format!("Connected to {one}"),
            (_, many) => format!("{} devices connected", many.len()),
        });

        w.off_group.set_visible(!on);
        if !st.service {
            w.off.set(
                "dmm-bluetooth-disabled-symbolic",
                "Bluetooth service isn't running",
                "Start it with “systemctl enable --now bluetooth”.",
            );
        } else if adapter.is_none() {
            w.off.set(
                "dmm-bluetooth-disabled-symbolic",
                "No Bluetooth adapter",
                "No Bluetooth hardware was found.",
            );
        } else {
            w.off.set(
                "dmm-bluetooth-disabled-symbolic",
                "Bluetooth is off",
                "Turn it on to connect headphones, mice, keyboards and more.",
            );
        }

        let item = |d: &Device| (d.path.clone(), (d.clone(), self.busy.contains(&d.path)));
        let paired = st.devices.iter().filter(|d| d.paired).map(item).collect();
        w.paired
            .sync(paired, |path| DeviceRow::new(path, true, &sender));
        w.paired_group.set_visible(on && !w.paired.is_empty());

        // Unnamed devices are mostly beacons and phones' randomized addresses: noise.
        let mut nearby: Vec<_> = st
            .devices
            .iter()
            .filter(|d| !d.paired && d.named)
            .map(item)
            .collect();
        nearby.sort_by_key(|(_, (d, _))| d.name.to_lowercase());
        w.nearby
            .sync(nearby, |path| DeviceRow::new(path, false, &sender));
        w.nearby_group.set_visible(on);
        w.searching
            .set_visible(adapter.is_some_and(|a| a.discovering));
    }
}

impl BluetoothPage {
    fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }

    fn powered(&self) -> bool {
        self.state.adapter.as_ref().is_some_and(|a| a.powered)
    }
}

/// Device types (BlueZ icon names) we bundle an icon for.
const DEVICE_ICONS: &[&str] = &[
    "audio-card",
    "audio-headphones",
    "audio-headset",
    "audio-speakers",
    "camera-photo",
    "camera-video",
    "computer",
    "input-gaming",
    "input-keyboard",
    "input-mouse",
    "input-tablet",
    "multimedia-player",
    "phone",
    "printer",
    "video-display",
];

fn device_icon(icon: Option<&str>) -> String {
    match icon {
        Some(icon) if DEVICE_ICONS.contains(&icon) => format!("dmm-{icon}-symbolic"),
        _ => "dmm-bluetooth-active-symbolic".to_string(),
    }
}

fn device_kind(icon: Option<&str>) -> &'static str {
    match icon.unwrap_or_default() {
        "audio-headset" => "Headset",
        "audio-headphones" => "Headphones",
        "audio-card" | "audio-speakers" => "Speaker",
        "input-mouse" => "Mouse",
        "input-keyboard" => "Keyboard",
        "input-gaming" => "Game controller",
        "input-tablet" => "Tablet",
        "phone" => "Phone",
        "computer" => "Computer",
        "video-display" => "Display",
        "camera-photo" | "camera-video" => "Camera",
        "printer" => "Printer",
        "multimedia-player" => "Media player",
        _ => "Device",
    }
}

struct DeviceRow {
    row: adw::ActionRow,
    icon: gtk::Image,
    spinner: adw::Spinner,
    menu: Option<gio::Menu>,
}

impl DeviceRow {
    fn new(path: &str, paired: bool, sender: &ComponentSender<BluetoothPage>) -> Self {
        let row = adw::ActionRow::builder()
            .use_markup(false)
            .activatable(true)
            .build();
        let icon = tile("dmm-bluetooth-active-symbolic");
        row.add_prefix(&icon);
        let spinner = adw::Spinner::builder().visible(false).build();
        row.add_suffix(&spinner);

        let s = sender.clone();
        let p = path.to_string();
        row.connect_activated(move |_| s.input(Input::Activate(p.clone())));

        let menu = paired.then(|| {
            let actions = gio::SimpleActionGroup::new();
            let toggle = gio::SimpleAction::new("toggle", None);
            let (s, p) = (sender.clone(), path.to_string());
            toggle.connect_activate(move |_, _| s.input(Input::Activate(p.clone())));
            let forget = gio::SimpleAction::new("forget", None);
            let (s, p) = (sender.clone(), path.to_string());
            forget.connect_activate(move |_, _| s.input(Input::ConfirmForget(p.clone())));
            actions.add_action(&toggle);
            actions.add_action(&forget);
            row.insert_action_group("device", Some(&actions));

            let menu = gio::Menu::new();
            let button = gtk::MenuButton::builder()
                .icon_name("dmm-view-more-symbolic")
                .menu_model(&menu)
                .css_classes(["flat", "circular"])
                .valign(gtk::Align::Center)
                .tooltip_text("More")
                .build();
            row.add_suffix(&button);
            menu
        });
        Self {
            row,
            icon,
            spinner,
            menu,
        }
    }
}

impl KeyedRow for DeviceRow {
    type Item = (Device, bool);

    fn widget(&self) -> &gtk::Widget {
        self.row.upcast_ref()
    }

    fn update(&self, (d, busy): &(Device, bool)) {
        self.row.set_title(&d.name);
        self.icon
            .set_icon_name(Some(&device_icon(d.icon.as_deref())));
        if d.connected {
            self.icon.add_css_class("accent");
        } else {
            self.icon.remove_css_class("accent");
        }
        let subtitle = match (d.paired, d.connected, *busy) {
            (false, _, true) => "Pairing…".to_string(),
            (false, _, false) => device_kind(d.icon.as_deref()).to_string(),
            (true, true, true) => "Disconnecting…".to_string(),
            (true, false, true) => "Connecting…".to_string(),
            (true, true, false) => match d.battery {
                Some(level) => format!("Connected · {level}% battery"),
                None => "Connected".to_string(),
            },
            (true, false, false) => "Not connected".to_string(),
        };
        self.row.set_subtitle(&subtitle);
        self.spinner.set_visible(*busy);

        if let Some(menu) = &self.menu {
            menu.remove_all();
            menu.append(
                Some(if d.connected { "Disconnect" } else { "Connect" }),
                Some("device.toggle"),
            );
            menu.append(Some("Forget"), Some("device.forget"));
        }
    }
}

/// Shows a pairing prompt from the agent; the answer goes back as `Input::Reply`.
fn prompt_dialog(
    parent: &adw::PreferencesPage,
    sender: &ComponentSender<BluetoothPage>,
    id: u32,
    device: &str,
    prompt: Prompt,
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(None, None);
    dialog.set_close_response("cancel");
    let code = |text: String| {
        gtk::Label::builder()
            .label(text)
            .selectable(true)
            .css_classes(["passkey"])
            .build()
    };
    let numeric = prompt == Prompt::Passkey;
    // Where a PIN or passkey is typed, if the prompt asks for one.
    let mut input: Option<adw::EntryRow> = None;

    match prompt {
        Prompt::Confirm { passkey } => {
            dialog.set_heading(Some(&format!("Pair with {device}?")));
            dialog.set_body(&format!("Check that {device} shows the same code."));
            dialog.set_extra_child(Some(&code(format!("{passkey:06}"))));
            dialog.add_responses(&[("cancel", "Cancel"), ("accept", "Pair")]);
        }
        Prompt::Authorize => {
            dialog.set_heading(Some(&format!("Pair with {device}?")));
            dialog.set_body(&format!("{device} wants to pair with this computer."));
            dialog.add_responses(&[("cancel", "Deny"), ("accept", "Allow")]);
        }
        Prompt::PinCode | Prompt::Passkey => {
            dialog.set_heading(Some(&format!("Pair with {device}")));
            dialog.set_body(if numeric {
                "Enter the 6-digit passkey shown on the device."
            } else {
                "Enter the device's PIN code (often 0000 or 1234)."
            });
            let entry = adw::EntryRow::builder()
                .title(if numeric { "Passkey" } else { "PIN code" })
                .build();
            if numeric {
                entry.set_input_purpose(gtk::InputPurpose::Digits);
            }
            let list = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            list.append(&entry);
            dialog.set_extra_child(Some(&list));
            dialog.add_responses(&[("cancel", "Cancel"), ("accept", "Pair")]);
            dialog.set_response_enabled("accept", false);
            let valid = move |text: &str| {
                if numeric {
                    text.len() <= 6 && text.parse::<u32>().is_ok()
                } else {
                    !text.is_empty()
                }
            };
            let d = dialog.clone();
            entry.connect_changed(move |e| d.set_response_enabled("accept", valid(&e.text())));
            input = Some(entry);
        }
        Prompt::DisplayPasskey { passkey } => {
            dialog.set_heading(Some(&format!("Pair with {device}")));
            dialog.set_body(&format!("Type this code on {device}, then press Enter."));
            dialog.set_extra_child(Some(&code(format!("{passkey:06}"))));
            dialog.add_responses(&[("cancel", "Close")]);
        }
        Prompt::DisplayPinCode { pin } => {
            dialog.set_heading(Some(&format!("Pair with {device}")));
            dialog.set_body(&format!("Type this PIN on {device}, then press Enter."));
            dialog.set_extra_child(Some(&code(pin)));
            dialog.add_responses(&[("cancel", "Close")]);
        }
    }
    let needs_reply = dialog.has_response("accept");
    if needs_reply {
        dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("accept"));
    }

    let s = sender.clone();
    dialog.connect_response(None, move |_, response| {
        if !needs_reply {
            return;
        }
        let text = input.as_ref().map(|e| e.text().to_string());
        let reply = match (response, text) {
            ("accept", None) => Reply::Accept,
            ("accept", Some(text)) if numeric => Reply::Passkey(text.parse().unwrap_or(0)),
            ("accept", Some(text)) => Reply::PinCode(text),
            _ => Reply::Reject,
        };
        s.input(Input::Reply { id, reply });
    });
    dialog.present(Some(parent));
    dialog
}
