//! Keyboard Shortcuts: niri's key binds. Binds from the user's own files are shown as they
//! are; new ones and edits go to dm-manager.kdl (a bind there with a key used elsewhere
//! replaces that one).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use dmm_sys::binds::{self, ACTIONS, Arg, BindSpec, Category};
use dmm_sys::niri::{self, Bind, InputConfig, NiriFile};
use relm4::adw::prelude::*;
use relm4::gtk::{gdk, glib};
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::keys;
use crate::widgets::{Hero, confirm, icon_button};

pub struct BindsPage {
    config: InputConfig,
    filter: String,
    /// The open editor, closed once its bind is saved.
    editor: Option<adw::Dialog>,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Filter(String),
    Add,
    Edit(String),
    ConfirmDelete(String),
    Delete(String),
    Save { old_key: Option<String>, spec: BindSpec },
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    add: gtk::Button,
    notice: adw::PreferencesGroup,
    groups: Vec<(Option<Category>, adw::PreferencesGroup, gtk::ListBox)>,
    empty: adw::PreferencesGroup,
}

impl Component for BindsPage {
    type CommandOutput = ();
    type Input = Input;
    type Output = Output;
    type Init = ();
    type Root = adw::PreferencesPage;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        adw::PreferencesPage::new()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let hero = Hero::new("dmm-preferences-desktop-keyboard-shortcuts-symbolic", "Keyboard Shortcuts");
        let add = gtk::Button::builder()
            .child(&adw::ButtonContent::builder().icon_name("dmm-list-add-symbolic").label("Add").build())
            .css_classes(["pill", "suggested-action"])
            .build();
        let s = sender.clone();
        add.connect_clicked(move |_| s.input(Input::Add));
        hero.add_suffix(&add);
        let search = gtk::SearchEntry::builder().placeholder_text("Search shortcuts").margin_top(12).build();
        let s = sender.clone();
        search.connect_search_changed(move |e| s.input(Input::Filter(e.text().to_string())));
        let group = adw::PreferencesGroup::new();
        group.add(&hero.root);
        group.add(&search);
        root.add(&group);

        let notice = adw::PreferencesGroup::new();
        let card = gtk::Label::builder()
            .label("niri doesn't load DM Manager's settings yet, so shortcuts made here wouldn't work. Connect it on the Keyboard &amp; Input page.")
            .use_markup(true)
            .wrap(true)
            .xalign(0.0)
            .css_classes(["card", "notice"])
            .build();
        notice.add(&card);
        root.add(&notice);

        let mut groups = Vec::new();
        for category in Category::ALL.map(Some).into_iter().chain([None]) {
            let title = category.map_or("Other", |c| c.label());
            let list =
                gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
            let group = adw::PreferencesGroup::builder().title(title.replace('&', "&amp;")).build();
            group.add(&list);
            root.add(&group);
            groups.push((category, group, list));
        }
        let empty = adw::PreferencesGroup::new();
        empty.add(
            &adw::StatusPage::builder().icon_name("dmm-edit-find-symbolic").title("No Matching Shortcuts").build(),
        );
        root.add(&empty);

        let model = Self { config: InputConfig::default(), filter: String::new(), editor: None };
        let mut widgets = Widgets { hero, add, notice, groups, empty };
        let mut model = model;
        model.reload(&mut widgets, &sender);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => self.reload(w, &sender),
            Input::Visible(false) => {}
            Input::Filter(text) => {
                self.filter = text.to_lowercase();
                self.show(w, &sender);
            }
            Input::Add => self.editor = Some(editor(root, &sender, &self.config, None)),
            Input::Edit(key) => {
                if let Some(bind) = self.config.binds.get(&key).filter(|b| b.own) {
                    self.editor = Some(editor(root, &sender, &self.config, Some(bind)));
                }
            }
            Input::ConfirmDelete(key) => {
                let Some(bind) = self.config.binds.get(&key) else { return };
                let s = sender.clone();
                let body = match &bind.overrides {
                    Some((source, action)) => {
                        format!("{key} goes back to what {} sets: {}.", relative(source), binds::describe(action))
                    }
                    None => format!("{key} won't do anything anymore."),
                };
                confirm(root, &format!("Delete {key}?"), &body, "Delete", move || s.input(Input::Delete(key.clone())));
            }
            Input::Delete(key) => {
                let result = NiriFile::load().and_then(|mut f| {
                    f.remove_bind(&key)?;
                    f.save()
                });
                match result {
                    Ok(()) => toast(&sender, format!("{key} deleted")),
                    Err(e) => toast(&sender, e),
                }
                if let Some(editor) = self.editor.take() {
                    editor.close();
                }
                self.reload(w, &sender);
            }
            Input::Save { old_key, spec } => {
                let result = NiriFile::load().and_then(|mut f| {
                    f.set_bind(old_key.as_deref(), &spec)?;
                    f.save()
                });
                match result {
                    Ok(()) => {
                        toast(&sender, format!("{} saved", spec.key));
                        if let Some(editor) = self.editor.take() {
                            editor.close();
                        }
                        self.reload(w, &sender);
                    }
                    // The editor stays open to fix it.
                    Err(e) => toast(&sender, format!("Not saved: {e}")),
                }
            }
        }
    }
}

impl BindsPage {
    fn reload(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        self.config = niri::effective();
        self.show(w, sender);
    }

    fn show(&self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        let c = &self.config;
        let own = c.binds.values().filter(|b| b.own).count();
        w.hero.set_subtitle(&format!(
            "{} shortcut{} · {own} made here",
            c.binds.len(),
            if c.binds.len() == 1 { "" } else { "s" }
        ));
        w.notice.set_visible(!c.included);
        w.add.set_sensitive(c.included);

        for (_, group, list) in &w.groups {
            list.remove_all();
            group.set_visible(false);
        }
        let mut shown = 0;
        let mut sorted: Vec<(String, &Bind)> = c.binds.values().map(|b| (bind_label(b), b)).collect();
        sorted.sort_by_key(|(label, _)| label.to_lowercase());
        for (label, bind) in sorted {
            let haystack = format!("{} {} {} {}", bind.key, label, bind.action, relative(&bind.source)).to_lowercase();
            if !self.filter.is_empty() && !self.filter.split_whitespace().all(|w| haystack.contains(w)) {
                continue;
            }
            let category = binds::parse(&bind.action).and_then(|p| p.action).map(|a| a.category);
            if let Some((_, group, list)) = w.groups.iter().find(|(c, ..)| *c == category) {
                list.append(&bind_row(bind, &label, sender));
                group.set_visible(true);
                shown += 1;
            }
        }
        w.empty.set_visible(shown == 0);
    }
}

fn toast(sender: &ComponentSender<BindsPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn bind_label(bind: &Bind) -> String {
    bind.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| binds::describe(&bind.action))
}

/// A path under niri's config directory, relative to it ("dm-desktop/binds.kdl").
fn relative(path: &Path) -> String {
    path.strip_prefix(niri::niri_dir()).unwrap_or(path).display().to_string()
}

/// A key combination as a row of keycaps.
fn keycaps(key: &str) -> gtk::Widget {
    let row = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
    for cap in keys::caps(key) {
        row.append(&gtk::Label::builder().label(&cap).css_classes(["keycap"]).build());
    }
    row.upcast()
}

fn bind_row(bind: &Bind, label: &str, sender: &ComponentSender<BindsPage>) -> adw::ActionRow {
    let subtitle = if bind.own {
        match &bind.overrides {
            Some((source, _)) => format!("Made here · replaces the one in {}", relative(source)),
            None => "Made here".to_string(),
        }
    } else {
        relative(&bind.source)
    };
    let row = adw::ActionRow::builder().title(label).subtitle(&subtitle).use_markup(false).build();
    row.add_suffix(&keycaps(&bind.key));
    if bind.own {
        row.set_activatable(true);
        let (s, key) = (sender.clone(), bind.key.clone());
        row.connect_activated(move |_| s.input(Input::Edit(key.clone())));
        let delete = icon_button("dmm-user-trash-symbolic", "Delete");
        let (s, key) = (sender.clone(), bind.key.clone());
        delete.connect_clicked(move |_| s.input(Input::ConfirmDelete(key.clone())));
        row.add_suffix(&delete);
    } else {
        row.set_tooltip_text(Some("Set in your niri config; DM Manager doesn't change it"));
        row.add_css_class("dim-row");
    }
    row
}

/// The hint for an action's argument field.
fn arg_title(arg: Arg) -> &'static str {
    match arg {
        Arg::None => "",
        Arg::Command => "Program and arguments",
        Arg::Shell => "Shell command",
        Arg::Number => "Number",
        Arg::Workspace => "Workspace number or name",
        Arg::Text(hint) => hint,
    }
}

/// The dialog to make or edit a bind. Saving sends `Input::Save`; the page closes it when
/// the bind is saved.
fn editor(
    parent: &adw::PreferencesPage,
    sender: &ComponentSender<BindsPage>,
    config: &InputConfig,
    existing: Option<&Bind>,
) -> adw::Dialog {
    let quiet = Rc::new(Cell::new(false));
    let key = Rc::new(RefCell::new(existing.map(|b| b.key.clone()).unwrap_or_default()));
    // The action was edited as raw text (or can only be).
    let custom = Rc::new(Cell::new(false));

    // Keys
    let caps = gtk::Box::builder().valign(gtk::Align::Center).build();
    let record = gtk::ToggleButton::builder()
        .label("Record")
        .valign(gtk::Align::Center)
        .css_classes(["pill", "small-pill"])
        .build();
    let keys_row = adw::ActionRow::builder().title("Keys").subtitle("Press Record, then the keys").build();
    keys_row.add_suffix(&caps);
    keys_row.add_suffix(&record);
    let key_entry = adw::EntryRow::builder().title("As niri writes it (e.g. Mod+Shift+T, Mod+WheelScrollDown)").build();
    let conflict = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["warning", "caption"])
        .margin_top(8)
        .build();
    let keys_group = adw::PreferencesGroup::builder().title("Shortcut").build();
    keys_group.add(&keys_row);
    keys_group.add(&key_entry);
    keys_group.add(&conflict);

    // Action
    let labels: Vec<String> = ACTIONS.iter().map(|a| a.label.to_string()).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let action_row =
        adw::ComboRow::builder().title("Action").model(&gtk::StringList::new(&labels)).enable_search(true).build();
    let arg_row = adw::EntryRow::new();
    let raw_row = adw::EntryRow::builder().title("niri action (advanced)").build();
    let action_group = adw::PreferencesGroup::builder().title("Action").build();
    action_group.add(&action_row);
    action_group.add(&arg_row);
    action_group.add(&raw_row);

    // Options
    let title_row = adw::EntryRow::builder().title("Name in the hotkey overlay (optional)").build();
    let repeat_row = adw::SwitchRow::builder().title("Repeat While Held").active(true).build();
    let locked_row = adw::SwitchRow::builder().title("Works on the Lock Screen").build();
    let options = adw::PreferencesGroup::builder().title("Options").build();
    options.add(&title_row);
    options.add(&repeat_row);
    options.add(&locked_row);

    let page = adw::PreferencesPage::new();
    page.add(&keys_group);
    page.add(&action_group);
    page.add(&options);
    if let Some(bind) = existing {
        let delete = adw::ButtonRow::builder().title("Delete Shortcut").css_classes(["destructive-action"]).build();
        let (s, k) = (sender.clone(), bind.key.clone());
        delete.connect_activated(move |_| s.input(Input::ConfirmDelete(k.clone())));
        let group = adw::PreferencesGroup::new();
        group.add(&delete);
        page.add(&group);
    }

    // Shows the key: keycaps, the text field, and whether it takes over another bind.
    let show_key = {
        let (caps, key_entry, conflict, quiet) = (caps.clone(), key_entry.clone(), conflict.clone(), quiet.clone());
        let binds = config.binds.clone();
        let editing = existing.map(|b| b.key.clone());
        move |k: &str| {
            while let Some(child) = caps.first_child() {
                caps.remove(&child);
            }
            if !k.is_empty() {
                caps.append(&keycaps(k));
            }
            if key_entry.text() != k {
                quiet.set(true);
                key_entry.set_text(k);
                quiet.set(false);
            }
            let taken = binds.get(k).filter(|_| editing.as_deref() != Some(k));
            conflict.set_visible(taken.is_some());
            if let Some(other) = taken {
                let whose =
                    if other.own { "made here".to_string() } else { format!("from {}", relative(&other.source)) };
                conflict.set_label(&format!("{k} already does “{}” ({whose}); this replaces it.", bind_label(other)));
            }
        }
    };
    let show_key = Rc::new(show_key);

    // Shows the action's argument field and the raw text built from the picker.
    let update_action = {
        let (action_row, arg_row, raw_row, quiet, custom) =
            (action_row.clone(), arg_row.clone(), raw_row.clone(), quiet.clone(), custom.clone());
        move || {
            let Some(action) = ACTIONS.get(action_row.selected() as usize) else { return };
            arg_row.set_visible(action.arg != Arg::None);
            arg_row.set_title(arg_title(action.arg));
            if custom.get() {
                return;
            }
            let built = binds::build(action, &arg_row.text()).unwrap_or_default();
            quiet.set(true);
            raw_row.set_text(&built);
            quiet.set(false);
        }
    };
    let update_action = Rc::new(update_action);

    // Fill in.
    quiet.set(true);
    let spawn = ACTIONS.iter().position(|a| a.id == "spawn").unwrap_or(0) as u32;
    action_row.set_selected(spawn);
    if let Some(bind) = existing {
        title_row.set_text(bind.title.as_deref().unwrap_or_default());
        repeat_row.set_active(bind.repeat);
        locked_row.set_active(bind.allow_when_locked);
        let parsed = binds::parse(&bind.action);
        match parsed.as_ref().and_then(|p| p.action.map(|a| (a, p))) {
            Some((action, p)) if !p.custom => {
                action_row.set_selected(ACTIONS.iter().position(|a| a.id == action.id).unwrap_or(0) as u32);
                arg_row.set_text(&p.arg);
            }
            _ => custom.set(true),
        }
        raw_row.set_text(&bind.action);
    }
    quiet.set(false);
    show_key(&key.borrow());
    update_action();

    {
        let (u, q, c) = (update_action.clone(), quiet.clone(), custom.clone());
        action_row.connect_selected_notify(move |_| {
            if !q.get() {
                c.set(false);
                u();
            }
        });
    }
    {
        let (u, q, c) = (update_action.clone(), quiet.clone(), custom.clone());
        arg_row.connect_changed(move |_| {
            if !q.get() {
                c.set(false);
                u();
            }
        });
    }
    {
        let (q, c) = (quiet.clone(), custom.clone());
        raw_row.connect_changed(move |_| {
            if !q.get() {
                c.set(true);
            }
        });
    }
    {
        let (k, show, q) = (key.clone(), show_key.clone(), quiet.clone());
        key_entry.connect_changed(move |e| {
            if !q.get() {
                *k.borrow_mut() = e.text().trim().to_string();
                show(&k.borrow());
            }
        });
    }

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::builder().show_end_title_buttons(false).show_start_title_buttons(false).build();
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::builder().label("Save").css_classes(["suggested-action"]).build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title(if existing.is_some() { "Edit Shortcut" } else { "New Shortcut" })
        .content_width(560)
        .content_height(680)
        .child(&toolbar)
        .build();

    // Recording: the next key press (with its modifiers) becomes the key. niri is asked to
    // pass its own shortcuts through meanwhile.
    let toplevel = parent.root().and_then(|r| r.surface()).and_downcast::<gdk::Toplevel>();
    {
        let toplevel = toplevel.clone();
        record.connect_toggled(move |b| {
            b.set_label(if b.is_active() { "Press keys…" } else { "Record" });
            if let Some(t) = &toplevel {
                if b.is_active() {
                    t.inhibit_system_shortcuts(None::<&gdk::Event>);
                } else {
                    t.restore_system_shortcuts();
                }
            }
        });
    }
    let keys_ctl = gtk::EventControllerKey::new();
    keys_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (record, k, show) = (record.clone(), key.clone(), show_key.clone());
        keys_ctl.connect_key_pressed(move |ctl, keyval, keycode, state| {
            if !record.is_active() {
                return glib::Propagation::Proceed;
            }
            if keyval == gdk::Key::Escape && state.is_empty() {
                record.set_active(false);
                return glib::Propagation::Stop;
            }
            let display =
                ctl.widget().map(|w| w.display()).unwrap_or_else(|| gdk::Display::default().expect("a display"));
            if let Some(name) = keys::from_event(&display, keyval, keycode, state) {
                *k.borrow_mut() = name;
                show(&k.borrow());
                record.set_active(false);
            }
            glib::Propagation::Stop
        });
    }
    dialog.add_controller(keys_ctl);
    {
        let toplevel = toplevel.clone();
        dialog.connect_closed(move |_| {
            if let Some(t) = &toplevel {
                t.restore_system_shortcuts();
            }
        });
    }

    let d = dialog.clone();
    cancel.connect_clicked(move |_| {
        d.close();
    });
    let old_key = existing.map(|b| b.key.clone());
    let s = sender.clone();
    save.connect_clicked(move |_| {
        let key = key.borrow().trim().to_string();
        if key.is_empty() {
            let _ = s.output(Output::Toast("Record or type the keys first".into()));
            return;
        }
        let action = if custom.get() {
            Ok(raw_row.text().trim().to_string())
        } else {
            ACTIONS
                .get(action_row.selected() as usize)
                .ok_or_else(|| "Pick an action".to_string())
                .and_then(|a| binds::build(a, &arg_row.text()))
        };
        let action = match action {
            Ok(a) if !a.is_empty() => a,
            Ok(_) => return drop(s.output(Output::Toast("Enter an action".into()))),
            Err(e) => return drop(s.output(Output::Toast(e))),
        };
        let title = title_row.text().trim().to_string();
        let spec = BindSpec {
            key,
            action,
            title: (!title.is_empty()).then_some(title),
            repeat: repeat_row.is_active(),
            allow_when_locked: locked_row.is_active(),
        };
        s.input(Input::Save { old_key: old_key.clone(), spec });
    });

    dialog.present(Some(parent));
    dialog
}
