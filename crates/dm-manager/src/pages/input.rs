//! Keyboard & Input: layouts and how to switch them, typing, touchpad, mouse and cursor.
//!
//! Shows what's in effect across all of niri's config files; changes go to dm-manager's own
//! file (`dm-manager.kdl`, included last), which niri reloads live.

use std::cell::RefCell;
use std::rc::Rc;

use dmm_sys::kdl_text::Value;
use dmm_sys::niri::{self, Area, CursorTheme, InputConfig, NiriFile, XkbCatalog};
use relm4::adw::prelude::*;
use relm4::gtk::{gdk, glib};
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::{
    Bindings, Debounce, Hero, KeyedList, KeyedRow, combo_row, icon_button, scale_row, spin_row, switch_row,
};

pub struct InputPage {
    effective: InputConfig,
    file: Option<NiriFile>,
    catalog: Rc<XkbCatalog>,
    save: Debounce,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    /// Sets a value (`None`: back to the other files or niri's default), then saves; `delayed`
    /// saves once input pauses (sliders and spin buttons).
    Set {
        area: Area,
        key: &'static str,
        value: Option<Value>,
        delayed: bool,
    },
    AddLayout(String, String),
    RemoveLayout(usize),
    MoveLayoutUp(usize),
    ShowAddLayout,
    /// The `grp:` option that switches layouts, or none.
    SwitchWith(Option<String>),
    CursorTheme(String),
    CursorSize(u32),
    AddInclude,
    Save,
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    include_group: adw::PreferencesGroup,
    settings: Vec<adw::PreferencesGroup>,
    layouts: KeyedList<LayoutRow>,
    bindings: Bindings<InputConfig>,
}

/// Choices for a combo row: label, and the value written (`None`: not set).
type Choices = &'static [(&'static str, Option<&'static str>)];

const ACCEL_PROFILES: Choices = &[("Default", None), ("Adaptive", Some("adaptive")), ("Flat", Some("flat"))];
const SCROLL_METHODS: Choices = &[
    ("Default", None),
    ("Two Fingers", Some("two-finger")),
    ("Edge", Some("edge")),
    ("While a Button Is Held", Some("on-button-down")),
    ("Off", Some("no-scroll")),
];
const CLICK_METHODS: Choices =
    &[("Default", None), ("Button Areas", Some("button-areas")), ("Number of Fingers", Some("clickfinger"))];
const CURSOR_SIZES: &[u32] = &[24, 32, 40, 48, 64, 96];
const HIDE_AFTER: &[(&str, Option<i64>)] = &[
    ("Never", None),
    ("1 second", Some(1000)),
    ("3 seconds", Some(3000)),
    ("5 seconds", Some(5000)),
    ("10 seconds", Some(10_000)),
];

impl Component for InputPage {
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
        let catalog = Rc::new(niri::xkb_catalog());
        let mut bindings = Bindings::new();

        let hero = Hero::new("dmm-input-keyboard-symbolic", "Keyboard & Input");
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);
        root.add(&hero_group);

        let include_group = include_group(&sender);
        root.add(&include_group);

        let mut settings = Vec::new();

        // Layouts
        let layouts = KeyedList::new(None);
        let add = adw::ButtonRow::builder().title("Add Layout…").start_icon_name("dmm-list-add-symbolic").build();
        let s = sender.clone();
        add.connect_activated(move |_| s.input(Input::ShowAddLayout));
        let add_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .margin_top(12)
            .build();
        add_list.append(&add);
        let group = adw::PreferencesGroup::builder()
            .title("Keyboard Layouts")
            .description("The first is the default; the shortcut cycles through them in this order")
            .build();
        group.add(&layouts.list);
        group.add(&add_list);
        root.add(&group);
        settings.push(group);

        let group = adw::PreferencesGroup::new();
        group.add(&switch_with_row(&catalog, &sender, &mut bindings));
        root.add(&group);
        settings.push(group);

        // Typing
        let group = adw::PreferencesGroup::builder().title("Typing").build();
        let delay = spin_row("Repeat Delay", "Milliseconds a key is held before it repeats", (100.0, 2000.0, 10.0), 0);
        bind_spin(&delay, Area::Keyboard, "repeat-delay", 600.0, &sender, &mut bindings);
        group.add(&delay);
        let rate = spin_row("Repeat Rate", "Characters per second while a key repeats", (1.0, 100.0, 1.0), 0);
        bind_spin(&rate, Area::Keyboard, "repeat-rate", 25.0, &sender, &mut bindings);
        group.add(&rate);
        let numlock = switch_row("Num Lock On at Start", "");
        bind_switch(&numlock, Area::Keyboard, "numlock", &sender, &mut bindings);
        group.add(&numlock);
        root.add(&group);
        settings.push(group);

        // Touchpad
        if niri::has_touchpad() {
            let group = adw::PreferencesGroup::builder().title("Touchpad").build();
            for (key, title, subtitle) in [
                ("tap", "Tap to Click", ""),
                ("natural-scroll", "Natural Scrolling", "Content moves with your fingers"),
                ("dwt", "Disable While Typing", ""),
                ("disabled-on-external-mouse", "Disable With a Mouse Connected", ""),
            ] {
                let row = switch_row(title, subtitle);
                bind_switch(&row, Area::Touchpad, key, &sender, &mut bindings);
                group.add(&row);
            }
            group.add(&speed_row(Area::Touchpad, &sender, &mut bindings));
            group.add(&choice_row(
                "Acceleration",
                "",
                Area::Touchpad,
                "accel-profile",
                ACCEL_PROFILES,
                &sender,
                &mut bindings,
            ));
            group.add(&choice_row(
                "Scrolling",
                "",
                Area::Touchpad,
                "scroll-method",
                SCROLL_METHODS,
                &sender,
                &mut bindings,
            ));
            group.add(&choice_row(
                "Right Click",
                "Click the bottom-right corner, or with two fingers",
                Area::Touchpad,
                "click-method",
                CLICK_METHODS,
                &sender,
                &mut bindings,
            ));
            root.add(&group);
            settings.push(group);
        }

        // Mouse
        let group = adw::PreferencesGroup::builder().title("Mouse").build();
        let natural = switch_row("Natural Scrolling", "Content moves with the wheel");
        bind_switch(&natural, Area::Mouse, "natural-scroll", &sender, &mut bindings);
        group.add(&natural);
        group.add(&speed_row(Area::Mouse, &sender, &mut bindings));
        group.add(&choice_row(
            "Acceleration",
            "",
            Area::Mouse,
            "accel-profile",
            ACCEL_PROFILES,
            &sender,
            &mut bindings,
        ));
        root.add(&group);
        settings.push(group);

        // Cursor
        let group =
            adw::PreferencesGroup::builder().title("Cursor").description("Also applied to GTK and Qt apps").build();
        group.add(&cursor_themes(&niri::cursor_themes(), &sender, &mut bindings));
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .margin_top(12)
            .build();
        list.append(&cursor_size_row(&sender, &mut bindings));
        let hide = switch_row("Hide While Typing", "");
        bind_switch(&hide, Area::Cursor, "hide-when-typing", &sender, &mut bindings);
        list.append(&hide);
        list.append(&hide_after_row(&sender, &mut bindings));
        group.add(&list);
        root.add(&group);
        settings.push(group);

        let mut model = Self { effective: InputConfig::default(), file: None, catalog, save: Debounce::new() };
        let mut widgets = Widgets { hero, include_group, settings, layouts, bindings };
        model.reload(&mut widgets, &sender);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => self.reload(w, &sender),
            Input::Visible(false) => {}
            Input::Set { area, key, value, delayed } => {
                self.set(&sender, area, key, value);
                if delayed {
                    let s = sender.clone();
                    self.save.run(300, move || s.input(Input::Save));
                } else {
                    self.save_now(w, &sender);
                }
            }
            Input::AddLayout(layout, variant) => {
                let mut layouts = self.effective.layouts();
                if !layouts.contains(&(layout.clone(), variant.clone())) {
                    layouts.push((layout, variant));
                    self.set_layouts(w, &sender, &layouts);
                }
            }
            Input::RemoveLayout(index) => {
                let mut layouts = self.effective.layouts();
                if layouts.len() > 1 && index < layouts.len() {
                    layouts.remove(index);
                    self.set_layouts(w, &sender, &layouts);
                }
            }
            Input::MoveLayoutUp(index) => {
                let mut layouts = self.effective.layouts();
                if index > 0 && index < layouts.len() {
                    layouts.swap(index - 1, index);
                    self.set_layouts(w, &sender, &layouts);
                }
            }
            Input::ShowAddLayout => add_layout_dialog(root, &self.catalog, &sender),
            Input::SwitchWith(option) => {
                let options = self.effective.xkb.string("options").unwrap_or_default();
                let value = niri::with_switch_option(options, option.as_deref()).map(Value::Str);
                self.set(&sender, Area::Xkb, "options", value);
                self.save_now(w, &sender);
            }
            Input::CursorTheme(theme) => {
                self.set(&sender, Area::Cursor, "xcursor-theme", Some(Value::Str(theme.clone())));
                self.save_now(w, &sender);
                std::thread::spawn(move || niri::apply_cursor_to_toolkits(Some(&theme), None));
            }
            Input::CursorSize(size) => {
                self.set(&sender, Area::Cursor, "xcursor-size", Some(Value::Int(size.into())));
                self.save_now(w, &sender);
                std::thread::spawn(move || niri::apply_cursor_to_toolkits(None, Some(size)));
            }
            Input::AddInclude => {
                match niri::add_include() {
                    Ok(()) => toast(&sender, "niri now loads DM Manager's settings".into()),
                    Err(e) => toast(&sender, e),
                }
                self.reload(w, &sender);
            }
            Input::Save => self.save_now(w, &sender),
        }
    }
}

impl InputPage {
    fn reload(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        self.effective = niri::effective();
        self.file = match NiriFile::load() {
            Ok(file) => Some(file),
            Err(e) => {
                toast(sender, e);
                None
            }
        };
        if let Some(e) = &self.effective.error {
            toast(sender, format!("Can't read niri's config: {e}"));
        }
        self.show(w, sender);
    }

    fn show(&self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        let c = &self.effective;
        w.include_group.set_visible(!c.included);
        for group in &w.settings {
            group.set_sensitive(c.included && self.file.is_some());
        }
        let layouts = c.layouts();
        let names: Vec<String> = layouts.iter().map(|(l, v)| self.catalog.describe(l, v)).collect();
        w.hero.set_subtitle(&if names.is_empty() {
            "Layout from the system (localectl)".to_string()
        } else {
            names.join(", ")
        });

        let count = layouts.len();
        let items = layouts
            .iter()
            .enumerate()
            .map(|(i, (l, v))| {
                let key = format!("{i}\n{l}\n{v}");
                (key, LayoutItem { index: i, count, name: self.catalog.describe(l, v), code: code(l, v) })
            })
            .collect();
        w.layouts.sync(items, |_| LayoutRow::new(sender));
        w.bindings.refresh(c);
    }

    fn set(&mut self, sender: &ComponentSender<Self>, area: Area, key: &str, value: Option<Value>) {
        let Some(file) = &mut self.file else { return };
        if let Err(e) = file.set(&self.effective, area, key, value) {
            toast(sender, e);
        }
    }

    fn set_layouts(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>, layouts: &[(String, String)]) {
        let (layout, variants) = niri::join_layouts(layouts);
        self.set(sender, Area::Xkb, "layout", Some(Value::Str(layout)));
        self.set(sender, Area::Xkb, "variant", variants.map(Value::Str));
        self.save_now(w, sender);
    }

    /// Saves (validated by niri), then shows what's now in effect.
    fn save_now(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        let Some(file) = &self.file else { return };
        if let Err(e) = file.save() {
            toast(sender, format!("Not saved: {e}"));
            // Drop the rejected edit.
            self.file = NiriFile::load().ok();
        }
        self.effective = niri::effective();
        self.show(w, sender);
    }
}

fn toast(sender: &ComponentSender<InputPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

/// `ru(phonetic)`, as xkb writes it.
fn code(layout: &str, variant: &str) -> String {
    if variant.is_empty() { layout.to_string() } else { format!("{layout}({variant})") }
}

fn include_group(sender: &ComponentSender<InputPage>) -> adw::PreferencesGroup {
    let title = gtk::Label::builder().label("Connect to niri").xalign(0.0).css_classes(["title-4"]).build();
    let body = gtk::Label::builder()
        .label(
            "DM Manager keeps its niri settings in dm-manager.kdl. niri loads that file once its config.kdl \
             includes it — this adds one line at the end:",
        )
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    let line = gtk::Label::builder()
        .label("include optional=true \"dm-manager.kdl\"")
        .xalign(0.0)
        .selectable(true)
        .css_classes(["monospace"])
        .build();
    let button = gtk::Button::builder()
        .label("Add to config.kdl")
        .halign(gtk::Align::Start)
        .css_classes(["pill", "suggested-action"])
        .build();
    let s = sender.clone();
    button.connect_clicked(move |_| s.input(Input::AddInclude));
    let card =
        gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(10).css_classes(["card", "notice"]).build();
    for w in [title.upcast_ref::<gtk::Widget>(), body.upcast_ref(), line.upcast_ref(), button.upcast_ref()] {
        card.append(w);
    }
    let group = adw::PreferencesGroup::new();
    group.add(&card);
    group
}

fn set_input(sender: &ComponentSender<InputPage>, area: Area, key: &'static str, value: Option<Value>, delayed: bool) {
    sender.input(Input::Set { area, key, value, delayed });
}

fn bind_switch(
    row: &adw::SwitchRow,
    area: Area,
    key: &'static str,
    sender: &ComponentSender<InputPage>,
    bindings: &mut Bindings<InputConfig>,
) {
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_active_notify(move |r| {
        if !quiet.get() {
            set_input(&s, area, key, Some(Value::Bool(r.is_active())), false);
        }
    });
    let r = row.clone();
    bindings.add(move |c| r.set_active(c.section(area).flag(key)));
}

fn bind_spin(
    row: &adw::SpinRow,
    area: Area,
    key: &'static str,
    default: f64,
    sender: &ComponentSender<InputPage>,
    bindings: &mut Bindings<InputConfig>,
) {
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_value_notify(move |r| {
        if !quiet.get() {
            set_input(&s, area, key, Some(Value::Int(r.value().round() as i64)), true);
        }
    });
    let r = row.clone();
    bindings.add(move |c| r.set_value(c.section(area).number(key).unwrap_or(default)));
}

fn speed_row(area: Area, sender: &ComponentSender<InputPage>, bindings: &mut Bindings<InputConfig>) -> adw::ActionRow {
    let (row, scale) = scale_row("Pointer Speed", "", (-1.0, 1.0, 0.05));
    scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    scale.connect_value_changed(move |sc| {
        if !quiet.get() {
            // Two decimals are plenty, and keep the file tidy.
            let v = (sc.value() * 100.0).round() / 100.0;
            set_input(&s, area, "accel-speed", Some(Value::Float(v)), true);
        }
    });
    bindings.add(move |c| scale.set_value(c.section(area).number("accel-speed").unwrap_or(0.0)));
    row
}

fn choice_row(
    title: &str,
    subtitle: &str,
    area: Area,
    key: &'static str,
    choices: Choices,
    sender: &ComponentSender<InputPage>,
    bindings: &mut Bindings<InputConfig>,
) -> adw::ComboRow {
    let labels: Vec<&str> = choices.iter().map(|(l, _)| *l).collect();
    let row = combo_row(title, subtitle, &labels);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_selected_notify(move |r| {
        if quiet.get() {
            return;
        }
        let value = choices.get(r.selected() as usize).and_then(|(_, v)| *v).map(|v| Value::Str(v.into()));
        set_input(&s, area, key, value, false);
    });
    let r = row.clone();
    bindings.add(move |c| {
        let current = c.section(area).string(key);
        let index = choices.iter().position(|(_, v)| *v == current).unwrap_or(0);
        r.set_selected(index as u32);
    });
    row
}

fn switch_with_row(
    catalog: &XkbCatalog,
    sender: &ComponentSender<InputPage>,
    bindings: &mut Bindings<InputConfig>,
) -> adw::ComboRow {
    let options: Rc<Vec<(String, String)>> =
        Rc::new(catalog.switch_options.iter().map(|o| (o.name.clone(), o.description.clone())).collect());
    let mut labels = vec!["Not Set"];
    labels.extend(options.iter().map(|(_, d)| d.as_str()));
    let row = combo_row("Switch Layouts With", "", &labels);
    row.set_enable_search(true);
    row.set_use_subtitle(false);
    let (s, quiet, opts) = (sender.clone(), bindings.quiet(), options.clone());
    row.connect_selected_notify(move |r| {
        if quiet.get() {
            return;
        }
        let option = (r.selected() as usize).checked_sub(1).and_then(|i| opts.get(i)).map(|(n, _)| n.clone());
        s.input(Input::SwitchWith(option));
    });
    let r = row.clone();
    bindings.add(move |c| {
        let grp = niri::switch_option(c.xkb.string("options").unwrap_or_default());
        let index = grp.and_then(|g| options.iter().position(|(n, _)| n == g)).map_or(0, |i| i + 1);
        r.set_selected(index as u32);
    });
    row
}

fn cursor_size_row(sender: &ComponentSender<InputPage>, bindings: &mut Bindings<InputConfig>) -> adw::ComboRow {
    let labels: Vec<String> = CURSOR_SIZES.iter().map(|s| s.to_string()).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let row = combo_row("Size", "", &labels);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_selected_notify(move |r| {
        if let (false, Some(size)) = (quiet.get(), CURSOR_SIZES.get(r.selected() as usize)) {
            s.input(Input::CursorSize(*size));
        }
    });
    let r = row.clone();
    bindings.add(move |c| {
        let size = c.cursor.number("xcursor-size").unwrap_or(24.0) as u32;
        // The closest offered size.
        let index = (0..CURSOR_SIZES.len()).min_by_key(|&i| CURSOR_SIZES[i].abs_diff(size)).unwrap_or(0);
        r.set_selected(index as u32);
    });
    row
}

fn hide_after_row(sender: &ComponentSender<InputPage>, bindings: &mut Bindings<InputConfig>) -> adw::ComboRow {
    let labels: Vec<&str> = HIDE_AFTER.iter().map(|(l, _)| *l).collect();
    let row = combo_row("Hide When Still", "After the pointer hasn't moved for", &labels);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_selected_notify(move |r| {
        if !quiet.get() {
            let value = HIDE_AFTER.get(r.selected() as usize).and_then(|(_, ms)| *ms).map(Value::Int);
            set_input(&s, Area::Cursor, "hide-after-inactive-ms", value, false);
        }
    });
    let r = row.clone();
    bindings.add(move |c| {
        let ms = c.cursor.number("hide-after-inactive-ms").map(|n| n as i64);
        let index = HIDE_AFTER.iter().position(|(_, v)| *v == ms).unwrap_or(0);
        r.set_selected(index as u32);
    });
    row
}

/// A grid of cursor themes, each previewed with a few of its cursors.
fn cursor_themes(
    themes: &[CursorTheme],
    sender: &ComponentSender<InputPage>,
    bindings: &mut Bindings<InputConfig>,
) -> gtk::FlowBox {
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(4)
        .row_spacing(10)
        .column_spacing(10)
        .css_classes(["cursor-themes"])
        .build();
    let children: Rc<RefCell<Vec<(String, gtk::FlowBoxChild)>>> = Rc::default();
    for theme in themes {
        let preview = gtk::Box::builder().spacing(6).halign(gtk::Align::Center).css_classes(["cursor-preview"]).build();
        for names in [&["default", "left_ptr"][..], &["pointer", "hand2"], &["text", "xterm"], &["wait", "watch"]] {
            if let Some(texture) = niri::cursor_image(theme, names, 32).and_then(cursor_texture) {
                let image = gtk::Image::from_paintable(Some(&texture));
                image.set_pixel_size(28);
                preview.append(&image);
            }
        }
        let name = gtk::Label::builder().label(&theme.name).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let card =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).css_classes(["cursor-card"]).build();
        card.append(&preview);
        card.append(&name);
        let child = gtk::FlowBoxChild::builder().child(&card).tooltip_text(&theme.id).build();
        flow.append(&child);
        children.borrow_mut().push((theme.id.clone(), child));
    }

    let (s, quiet, kids) = (sender.clone(), bindings.quiet(), children.clone());
    flow.connect_selected_children_changed(move |f| {
        if quiet.get() {
            return;
        }
        let Some(selected) = f.selected_children().into_iter().next() else { return };
        if let Some((id, _)) = kids.borrow().iter().find(|(_, c)| *c == selected) {
            s.input(Input::CursorTheme(id.clone()));
        }
    });
    let f = flow.clone();
    bindings.add(move |c| {
        let current = c.cursor.string("xcursor-theme").unwrap_or("default");
        match children.borrow().iter().find(|(id, _)| id == current) {
            Some((_, child)) => f.select_child(child),
            None => f.unselect_all(),
        }
    });
    flow
}

fn cursor_texture(image: niri::CursorImage) -> Option<gdk::MemoryTexture> {
    let stride = image.width as usize * 4;
    Some(gdk::MemoryTexture::new(
        i32::try_from(image.width).ok()?,
        i32::try_from(image.height).ok()?,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from_owned(image.pixels),
        stride,
    ))
}

/// A searchable list of every layout and variant xkb knows.
fn add_layout_dialog(parent: &adw::PreferencesPage, catalog: &XkbCatalog, sender: &ComponentSender<InputPage>) {
    let dialog = adw::Dialog::builder().title("Add Layout").content_width(440).content_height(600).build();
    let search = gtk::SearchEntry::builder().placeholder_text("Search layouts").hexpand(true).build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .valign(gtk::Align::Start)
        .build();

    let mut entries: Vec<(String, String, String)> = Vec::new();
    for layout in &catalog.layouts {
        entries.push((layout.name.clone(), String::new(), layout.description.clone()));
        for (variant, description) in &layout.variants {
            entries.push((layout.name.clone(), variant.clone(), description.clone()));
        }
    }
    for (layout, variant, description) in entries {
        let row = adw::ActionRow::builder()
            .title(&description)
            .subtitle(code(&layout, &variant))
            .use_markup(false)
            .activatable(true)
            .build();
        let (s, d) = (sender.clone(), dialog.clone());
        row.connect_activated(move |_| {
            s.input(Input::AddLayout(layout.clone(), variant.clone()));
            d.close();
        });
        list.append(&row);
    }

    let query = search.clone();
    list.set_filter_func(move |row| {
        let text = query.text().to_lowercase();
        if text.is_empty() {
            return true;
        }
        let Some(row) = row.downcast_ref::<adw::ActionRow>() else { return true };
        let title = row.title().to_lowercase();
        let subtitle = row.subtitle().map(|s| s.to_lowercase()).unwrap_or_default();
        text.split_whitespace().all(|word| title.contains(word) || subtitle.contains(word))
    });
    let l = list.clone();
    search.connect_search_changed(move |_| l.invalidate_filter());

    let search_bar = gtk::Box::builder().margin_start(12).margin_end(12).margin_bottom(6).build();
    search_bar.append(&search);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.add_top_bar(&search_bar);
    view.set_content(Some(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build()));
    dialog.set_child(Some(&view));
    dialog.present(Some(parent));
    search.grab_focus();
}

struct LayoutItem {
    index: usize,
    count: usize,
    name: String,
    code: String,
}

struct LayoutRow {
    row: adw::ActionRow,
    index: Rc<std::cell::Cell<usize>>,
    up: gtk::Button,
    remove: gtk::Button,
}

impl LayoutRow {
    fn new(sender: &ComponentSender<InputPage>) -> Self {
        let row = adw::ActionRow::builder().use_markup(false).build();
        let index = Rc::new(std::cell::Cell::new(0));
        let up = icon_button("dmm-go-up-symbolic", "Move up");
        let remove = icon_button("dmm-user-trash-symbolic", "Remove");
        let (s, i) = (sender.clone(), index.clone());
        up.connect_clicked(move |_| s.input(Input::MoveLayoutUp(i.get())));
        let (s, i) = (sender.clone(), index.clone());
        remove.connect_clicked(move |_| s.input(Input::RemoveLayout(i.get())));
        row.add_suffix(&up);
        row.add_suffix(&remove);
        Self { row, index, up, remove }
    }
}

impl KeyedRow for LayoutRow {
    type Item = LayoutItem;

    fn widget(&self) -> &gtk::Widget {
        self.row.upcast_ref()
    }

    fn update(&self, item: &LayoutItem) {
        self.index.set(item.index);
        self.row.set_title(&item.name);
        let subtitle = if item.index == 0 { format!("{} · default", item.code) } else { item.code.clone() };
        self.row.set_subtitle(&subtitle);
        self.up.set_visible(item.index > 0);
        self.remove.set_visible(item.count > 1);
    }
}
