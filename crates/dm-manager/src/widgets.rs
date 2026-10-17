//! Building blocks shared by the pages.

use std::collections::{HashMap, HashSet};

use relm4::adw::prelude::*;
use relm4::gtk::{self, glib};

/// The card at the top of a page: icon tile, title, status line and an on/off switch.
pub struct Hero {
    pub root: gtk::Box,
    icon: gtk::Image,
    subtitle: gtk::Label,
    switch: gtk::Switch,
    handler: glib::SignalHandlerId,
}

impl Hero {
    pub fn new(icon: &str, title: &str, on_toggle: impl Fn(bool) + 'static) -> Self {
        let icon = tile_with_size(icon, "hero-icon", 28);
        let title = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["hero-title"])
            .build();
        let subtitle = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["hero-subtitle"])
            .build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        text.append(&title);
        text.append(&subtitle);

        let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let handler = switch.connect_state_set(move |_, on| {
            on_toggle(on);
            glib::Propagation::Proceed
        });

        let root = gtk::Box::builder()
            .spacing(16)
            .css_classes(["hero"])
            .build();
        root.append(&icon);
        root.append(&text);
        root.append(&switch);
        Self {
            root,
            icon,
            subtitle,
            switch,
            handler,
        }
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
        if self.switch.is_active() != active {
            self.switch.block_signal(&self.handler);
            self.switch.set_active(active);
            self.switch.unblock_signal(&self.handler);
        }
        self.switch.set_sensitive(sensitive);
    }
}

/// A rounded icon tile for the start of a row.
pub fn tile(icon: &str) -> gtk::Image {
    tile_with_size(icon, "tile", 18)
}

/// The icon is drawn centered in the image's (CSS-sized, rounded) box.
fn tile_with_size(icon: &str, class: &str, size: i32) -> gtk::Image {
    gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(size)
        .css_classes([class])
        .valign(gtk::Align::Center)
        .build()
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
        let icon = gtk::Image::builder()
            .pixel_size(48)
            .css_classes(["dim-label"])
            .build();
        let title = gtk::Label::builder()
            .css_classes(["title-3"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        let body = gtk::Label::builder()
            .css_classes(["dim-label"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["card", "off-card"])
            .build();
        root.append(&icon);
        root.append(&title);
        root.append(&body);
        Self {
            root,
            icon,
            title,
            body,
        }
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
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        if let Some(text) = placeholder {
            let label = gtk::Label::builder()
                .label(text)
                .wrap(true)
                .css_classes(["empty-label"])
                .build();
            list.set_placeholder(Some(&label));
        }
        Self {
            list,
            rows: HashMap::new(),
            order: Vec::new(),
        }
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
            self.rows
                .entry(key.clone())
                .or_insert_with(|| create(key))
                .update(item);
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
