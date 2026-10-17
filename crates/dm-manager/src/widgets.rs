//! Building blocks shared by the pages.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use relm4::adw::prelude::*;
use relm4::gtk::{self, glib};

/// The card at the top of a page: icon tile, title, status line and optionally an on/off
/// switch or other controls.
pub struct Hero {
    pub root: gtk::Box,
    icon: gtk::Image,
    subtitle: gtk::Label,
    switch: Option<(gtk::Switch, glib::SignalHandlerId)>,
}

impl Hero {
    pub fn new(icon: &str, title: &str) -> Self {
        let icon = tile_with_size(icon, "hero-icon", 28);
        let title = gtk::Label::builder().label(title).xalign(0.0).css_classes(["hero-title"]).build();
        let subtitle = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["hero-subtitle"]).build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        text.append(&title);
        text.append(&subtitle);

        let root = gtk::Box::builder().spacing(16).css_classes(["hero"]).build();
        root.append(&icon);
        root.append(&text);
        Self { root, icon, subtitle, switch: None }
    }

    /// Adds an on/off switch at the end.
    pub fn with_switch(mut self, on_toggle: impl Fn(bool) + 'static) -> Self {
        let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let handler = switch.connect_state_set(move |_, on| {
            on_toggle(on);
            glib::Propagation::Proceed
        });
        self.root.append(&switch);
        self.switch = Some((switch, handler));
        self
    }

    /// Adds a control (e.g. a button) at the end.
    pub fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        widget.set_valign(gtk::Align::Center);
        self.root.append(widget);
    }

    pub fn set_icon(&self, icon: &str) {
        self.icon.set_icon_name(Some(icon));
    }

    pub fn set_subtitle(&self, text: &str) {
        self.subtitle.set_label(text);
    }

    /// Dims the card when the thing it controls is off or unavailable.
    pub fn set_on(&self, on: bool) {
        if on {
            self.root.remove_css_class("off");
        } else {
            self.root.add_css_class("off");
        }
    }

    /// Moves the switch without reporting it as a toggle.
    pub fn set_switch(&self, active: bool, sensitive: bool) {
        let Some((switch, handler)) = &self.switch else { return };
        if switch.is_active() != active {
            switch.block_signal(handler);
            switch.set_active(active);
            switch.unblock_signal(handler);
        }
        switch.set_sensitive(sensitive);
    }
}

/// A rounded icon tile for the start of a row.
pub fn tile(icon: &str) -> gtk::Image {
    tile_with_size(icon, "tile", 18)
}

/// The icon is drawn centered in the image's (CSS-sized, rounded) box.
fn tile_with_size(icon: &str, class: &str, size: i32) -> gtk::Image {
    gtk::Image::builder().icon_name(icon).pixel_size(size).css_classes([class]).valign(gtk::Align::Center).build()
}

/// A card shown instead of a page's lists when its feature is off or missing.
pub struct OffCard {
    pub root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    body: gtk::Label,
}

impl OffCard {
    pub fn new() -> Self {
        let icon = gtk::Image::builder().pixel_size(48).css_classes(["dim-label"]).build();
        let title =
            gtk::Label::builder().css_classes(["title-3"]).wrap(true).justify(gtk::Justification::Center).build();
        let body =
            gtk::Label::builder().css_classes(["dim-label"]).wrap(true).justify(gtk::Justification::Center).build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["card", "off-card"])
            .build();
        root.append(&icon);
        root.append(&title);
        root.append(&body);
        Self { root, icon, title, body }
    }

    pub fn set(&self, icon: &str, title: &str, body: &str) {
        self.icon.set_icon_name(Some(icon));
        self.title.set_label(title);
        self.body.set_label(body);
    }
}

/// A row in a [`KeyedList`].
pub trait KeyedRow {
    type Item;
    fn widget(&self) -> &gtk::Widget;
    fn update(&self, item: &Self::Item);
}

/// A boxed list whose rows are kept per key and updated in place, so a refresh doesn't
/// rebuild rows (and lose hover, focus or a running spinner).
pub struct KeyedList<R: KeyedRow> {
    pub list: gtk::ListBox,
    rows: HashMap<String, R>,
    order: Vec<String>,
}

impl<R: KeyedRow> KeyedList<R> {
    pub fn new(placeholder: Option<&str>) -> Self {
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        if let Some(text) = placeholder {
            let label = gtk::Label::builder().label(text).wrap(true).css_classes(["empty-label"]).build();
            list.set_placeholder(Some(&label));
        }
        Self { list, rows: HashMap::new(), order: Vec::new() }
    }

    pub fn sync(&mut self, items: Vec<(String, R::Item)>, create: impl Fn(&str) -> R) {
        let keys: Vec<String> = items.iter().map(|(k, _)| k.clone()).collect();
        let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
        let list = &self.list;
        self.rows.retain(|key, row| {
            let keep = wanted.contains(key.as_str());
            if !keep {
                list.remove(row.widget());
            }
            keep
        });
        for (key, item) in &items {
            self.rows.entry(key.clone()).or_insert_with(|| create(key)).update(item);
        }
        if keys != self.order {
            for key in &keys {
                let widget = self.rows[key].widget();
                if widget.parent().is_some() {
                    list.remove(widget);
                }
            }
            for key in &keys {
                list.append(self.rows[key].widget());
            }
            self.order = keys;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Asks to confirm a destructive action; `on_confirm` runs only if confirmed.
pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    action: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = relm4::adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", action)]);
    dialog.set_response_appearance("confirm", relm4::adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.connect_response(None, move |_, response| {
        if response == "confirm" {
            on_confirm();
        }
    });
    dialog.present(Some(parent));
}

type Refresh<T> = Box<dyn Fn(&T)>;

/// Keeps controls in step with a config without their change handlers firing: each control
/// registers how to show the config, and handlers ignore changes while [`Bindings::refresh`] runs.
pub struct Bindings<T> {
    quiet: Rc<Cell<bool>>,
    refreshers: Vec<Refresh<T>>,
}

impl<T> Bindings<T> {
    pub fn new() -> Self {
        Self { quiet: Rc::new(Cell::new(false)), refreshers: Vec::new() }
    }

    /// A flag for handlers: `true` while refreshing, when changes aren't the user's.
    pub fn quiet(&self) -> Rc<Cell<bool>> {
        self.quiet.clone()
    }

    pub fn add(&mut self, refresh: impl Fn(&T) + 'static) {
        self.refreshers.push(Box::new(refresh));
    }

    pub fn refresh(&self, config: &T) {
        self.quiet.set(true);
        for refresh in &self.refreshers {
            refresh(config);
        }
        self.quiet.set(false);
    }
}

pub fn switch_row(title: &str, subtitle: &str) -> relm4::adw::SwitchRow {
    relm4::adw::SwitchRow::builder().title(title).subtitle(subtitle).build()
}

pub fn spin_row(title: &str, subtitle: &str, (min, max, step): (f64, f64, f64), digits: u32) -> relm4::adw::SpinRow {
    let row = relm4::adw::SpinRow::with_range(min, max, step);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_digits(digits);
    row
}

/// A row with a choice of labels.
pub fn combo_row(title: &str, subtitle: &str, labels: &[&str]) -> relm4::adw::ComboRow {
    relm4::adw::ComboRow::builder().title(title).subtitle(subtitle).model(&gtk::StringList::new(labels)).build()
}

/// A row with a slider.
pub fn scale_row(
    title: &str,
    subtitle: &str,
    (min, max, step): (f64, f64, f64),
) -> (relm4::adw::ActionRow, gtk::Scale) {
    let row = relm4::adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    scale.set_width_request(200);
    scale.set_valign(gtk::Align::Center);
    scale.set_draw_value(false);
    row.add_suffix(&scale);
    (row, scale)
}

/// A flat round button showing an icon.
pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build()
}

/// Runs `then` once input has paused for `delay` (sliders and spin buttons fire per step).
pub struct Debounce {
    pending: Rc<Cell<Option<glib::SourceId>>>,
}

impl Debounce {
    pub fn new() -> Self {
        Self { pending: Rc::new(Cell::new(None)) }
    }

    pub fn run(&self, delay_ms: u64, then: impl FnOnce() + 'static) {
        if let Some(id) = self.pending.take() {
            id.remove();
        }
        let pending = self.pending.clone();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(delay_ms), move || {
            pending.set(None);
            then();
        });
        self.pending.set(Some(id));
    }
}
