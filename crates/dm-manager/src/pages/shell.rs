//! Shell: dmsh's config, with a live preview of the islands.
//!
//! Every change is previewed at once (rendered by `dmsh render` from the edited text), and
//! saved shortly after, when dmsh picks it up live. Feature switches need a restart.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use dmm_sys::dmsh::{self, DmshConfig, Kind, Preview, SETTINGS, Setting};
use dmm_sys::kdl_text::Value;
use dmm_sys::niri;
use relm4::adw::prelude::*;
use relm4::gtk::{gdk, pango};
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::{Bindings, Debounce, Hero, OffCard, icon_button, spin_row, switch_row};

pub struct ShellPage {
    config: Option<DmshConfig>,
    /// Edits not saved yet (a reload from disk would lose them).
    dirty: bool,
    restart_needed: bool,
    running: bool,
    preview: Preview,
    preview_gen: u64,
    save: Debounce,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Set(usize, Value),
    Reset(usize),
    Animations(bool),
    HiddenOutputs(Vec<String>),
    PreviewMode(Preview),
    Save,
    Restart,
}

#[derive(Debug)]
pub enum Cmd {
    Preview { generation: u64, result: Result<PathBuf, String> },
    Running(bool),
    Restarted(Result<(), String>),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    restart: gtk::Button,
    off_group: adw::PreferencesGroup,
    off: OffCard,
    groups: Vec<adw::PreferencesGroup>,
    picture: gtk::Picture,
    bindings: Bindings<DmshConfig>,
}

/// A group of rows on the page, and which settings go in it.
struct Group {
    title: &'static str,
    description: &'static str,
    includes: fn(&Setting) -> bool,
}

const fn group(title: &'static str, description: &'static str, includes: fn(&Setting) -> bool) -> Group {
    Group { title, description, includes }
}

const GROUPS: &[Group] = &[
    group("Look", "", |s| s.section == "theme" && s.prop.is_none() && matches!(s.kind, Kind::Color | Kind::Font)),
    group("Icon Colors", "By how important an activity is", |s| s.prop.is_some()),
    group("Size", "In logical pixels", |s| s.section == "theme" && matches!(s.kind, Kind::Number { .. })),
    group("Clock", "", |s| s.section == "clock"),
    group("Motion", "", |s| s.section == "animation"),
    group("Timing", "", |s| s.section == "timing"),
    group("Features", "Turning these on or off takes effect after restarting dmsh", |s| s.section == "sources"),
    group("Session Menu", "The power button on the right island", |s| s.section == "session"),
];

impl Component for ShellPage {
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
        let hero = Hero::new("dmm-shell-symbolic", "Shell");
        let restart = gtk::Button::builder().label("Restart dmsh").css_classes(["pill"]).build();
        let s = sender.clone();
        restart.connect_clicked(move |_| s.input(Input::Restart));
        hero.add_suffix(&restart);
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);
        root.add(&hero_group);

        let off = OffCard::new();
        let off_group = adw::PreferencesGroup::new();
        off_group.add(&off.root);
        root.add(&off_group);

        let mut bindings = Bindings::new();
        let mut groups = Vec::new();

        let (preview_group, picture) = preview_group(&sender);
        root.add(&preview_group);
        groups.push(preview_group);

        for Group { title, description, includes } in GROUPS {
            let group = adw::PreferencesGroup::builder().title(*title).description(*description).build();
            for (index, setting) in SETTINGS.iter().enumerate().filter(|(_, s)| includes(s)) {
                group.add(&setting_row(index, setting, &sender, &mut bindings));
            }
            if *title == "Motion" {
                group.add(&animations_row(&sender, &mut bindings));
            }
            if *title == "Clock" {
                // The description shows the formats applied to the current time.
                let g = group.clone();
                bindings.add(move |c: &DmshConfig| g.set_description(Some(&clock_example(c))));
            }
            root.add(&group);
            groups.push(group.clone());
            if *title == "Timing" {
                let displays = displays_group(&sender, &mut bindings);
                root.add(&displays);
                groups.push(displays);
            }
        }

        let model = Self {
            config: None,
            dirty: false,
            restart_needed: false,
            running: false,
            preview: Preview::Idle,
            preview_gen: 0,
            save: Debounce::new(),
        };
        let mut widgets = Widgets { hero, restart, off_group, off, groups, picture, bindings };
        model.update_view(&mut widgets, sender);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => {
                if !self.dirty {
                    self.reload(w, &sender);
                }
                check_running(&sender);
            }
            Input::Visible(false) => {}
            Input::Set(index, value) => {
                let setting = &SETTINGS[index];
                self.edit(&sender, |c| c.set(setting, &value));
                if setting.section == "sources" {
                    self.restart_needed = true;
                }
                // Only the reset buttons and the clock example change; the row shows the value.
                if let Some(c) = &self.config {
                    w.bindings.refresh(c);
                }
            }
            Input::Reset(index) => {
                self.edit(&sender, |c| c.reset(&SETTINGS[index]));
                if let Some(c) = &self.config {
                    w.bindings.refresh(c);
                }
            }
            Input::Animations(on) => self.edit(&sender, |c| c.set_animations(on)),
            Input::HiddenOutputs(hidden) => self.edit(&sender, |c| c.set_hidden_outputs(&hidden)),
            Input::PreviewMode(preview) => {
                self.preview = preview;
                self.render(&sender);
            }
            Input::Save => {
                if let Some(c) = &self.config {
                    match c.save() {
                        Ok(()) => self.dirty = false,
                        Err(e) => toast(&sender, format!("Not saved: {e}")),
                    }
                }
            }
            Input::Restart => {
                w.restart.set_sensitive(false);
                // Save first so the new instance starts with the latest settings.
                if self.dirty {
                    sender.input(Input::Save);
                }
                sender.oneshot_command(async {
                    let result =
                        tokio::task::spawn_blocking(dmsh::restart).await.unwrap_or_else(|e| Err(e.to_string()));
                    Cmd::Restarted(result)
                });
            }
        }
        let _ = root;
        self.update_view(w, sender);
    }

    fn update_cmd_with_view(&mut self, w: &mut Widgets, cmd: Cmd, sender: ComponentSender<Self>, _: &Self::Root) {
        match cmd {
            Cmd::Preview { generation, result } => {
                if generation != self.preview_gen {
                    return;
                }
                match result.and_then(|path| gdk::Texture::from_filename(&path).map_err(|e| e.to_string())) {
                    Ok(texture) => w.picture.set_paintable(Some(&texture)),
                    Err(e) => log::warn!("preview: {e}"),
                }
            }
            Cmd::Running(running) => self.running = running,
            Cmd::Restarted(result) => {
                w.restart.set_sensitive(true);
                match result {
                    Ok(()) => {
                        self.restart_needed = false;
                        toast(&sender, "dmsh restarted".into());
                    }
                    Err(e) => toast(&sender, format!("Couldn't restart dmsh: {e}")),
                }
                // Give it a moment to come up before checking.
                sender.oneshot_command(async {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    Cmd::Running(tokio::task::spawn_blocking(dmsh::is_running).await.unwrap_or(false))
                });
            }
        }
        self.update_view(w, sender);
    }

    fn update_view(&self, w: &mut Widgets, _: ComponentSender<Self>) {
        let installed = dmsh::binary().is_some();
        let usable = installed && self.config.is_some();
        w.off_group.set_visible(!usable);
        for group in &w.groups {
            group.set_visible(usable);
        }
        w.restart.set_visible(installed);
        w.restart.set_label(if self.running { "Restart dmsh" } else { "Start dmsh" });
        if self.restart_needed {
            w.restart.add_css_class("suggested-action");
        } else {
            w.restart.remove_css_class("suggested-action");
        }
        w.hero.set_on(usable);
        w.hero.set_subtitle(&if !installed {
            "dmsh isn't installed".to_string()
        } else if self.restart_needed {
            "Restart dmsh to apply the feature changes".to_string()
        } else if self.running {
            "dmsh is running · changes apply as you make them".to_string()
        } else {
            "dmsh isn't running".to_string()
        });
        if !installed {
            w.off.set(
                "dmm-shell-symbolic",
                "dmsh isn't installed",
                "Install it from github.com/Kexfff/DM-Shell to configure it here.",
            );
        } else if self.config.is_none() {
            w.off.set(
                "dmm-shell-symbolic",
                "Can't read dmsh's config",
                &DmshConfig::default_path().display().to_string(),
            );
        }
    }
}

impl ShellPage {
    fn reload(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        match DmshConfig::load(&DmshConfig::default_path()) {
            Ok(config) => {
                w.bindings.refresh(&config);
                self.config = Some(config);
                self.render(sender);
            }
            Err(e) => {
                self.config = None;
                toast(sender, e);
            }
        }
    }

    /// Changes the config in memory, previews it, and saves it once edits pause.
    fn edit(&mut self, sender: &ComponentSender<Self>, change: impl FnOnce(&mut DmshConfig) -> Result<(), String>) {
        let Some(config) = &mut self.config else { return };
        if let Err(e) = change(config) {
            toast(sender, e);
            return;
        }
        self.dirty = true;
        self.render(sender);
        let s = sender.clone();
        self.save.run(400, move || s.input(Input::Save));
    }

    fn render(&mut self, sender: &ComponentSender<Self>) {
        let Some(config) = &self.config else { return };
        self.preview_gen += 1;
        let generation = self.preview_gen;
        let text = config.text().to_string();
        let mode = self.preview;
        // Two files in turn, so a render never overwrites the one being loaded.
        let path = preview_dir().join(format!("preview-{}.png", generation % 2));
        sender.oneshot_command(async move {
            let result = tokio::task::spawn_blocking(move || dmsh::render_preview(&text, mode, &path).map(|()| path))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            Cmd::Preview { generation, result }
        });
    }
}

fn toast(sender: &ComponentSender<ShellPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn check_running(sender: &ComponentSender<ShellPage>) {
    sender
        .oneshot_command(async { Cmd::Running(tokio::task::spawn_blocking(dmsh::is_running).await.unwrap_or(false)) });
}

fn preview_dir() -> PathBuf {
    let dir =
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("dm-manager");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The islands as they'd look, on a backdrop, with a choice of what they show.
fn preview_group(sender: &ComponentSender<ShellPage>) -> (adw::PreferencesGroup, gtk::Picture) {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::ScaleDown)
        .can_shrink(true)
        .valign(gtk::Align::Start)
        .build();
    let backdrop = gtk::Box::builder().css_classes(["preview"]).build();
    backdrop.append(&picture);

    let modes = adw::ToggleGroup::new();
    for (name, label) in [("idle", "Idle"), ("center", "Center"), ("right", "Controls")] {
        modes.add(adw::Toggle::builder().name(name).label(label).build());
    }
    modes.set_active_name(Some("idle"));
    modes.set_valign(gtk::Align::Center);
    let s = sender.clone();
    modes.connect_active_name_notify(move |g| {
        let mode = match g.active_name().as_deref() {
            Some("center") => Preview::Center,
            Some("right") => Preview::Right,
            _ => Preview::Idle,
        };
        s.input(Input::PreviewMode(mode));
    });

    let group = adw::PreferencesGroup::builder().title("Preview").header_suffix(&modes).build();
    group.add(&backdrop);
    (group, picture)
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
fn hex(c: &gdk::RGBA) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let rgb = format!("#{:02x}{:02x}{:02x}", byte(c.red()), byte(c.green()), byte(c.blue()));
    if byte(c.alpha()) == 255 { rgb } else { format!("{rgb}{:02x}", byte(c.alpha())) }
}

fn clock_example(c: &DmshConfig) -> String {
    let format = |key| {
        let s = SETTINGS.iter().find(|s| s.section == "clock" && s.key == key).expect("clock setting");
        c.value(s).and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
    };
    let now = gtk::glib::DateTime::now_local().ok();
    let show = |f: String| now.as_ref().and_then(|n| n.format(&f).ok()).map(|s| s.to_string()).unwrap_or(f);
    format!("Now: {} · {}", show(format("format")), show(format("date-format")))
}

/// A row for one setting, wired to `Input::Set`/`Input::Reset`, with a reset button that
/// shows while the file sets a value.
fn setting_row(
    index: usize,
    s: &'static Setting,
    sender: &ComponentSender<ShellPage>,
    bindings: &mut Bindings<DmshConfig>,
) -> gtk::Widget {
    let quiet = bindings.quiet();
    let reset = icon_button("dmm-edit-undo-symbolic", "Reset to default");
    let r = reset.clone();
    let shows_reset = move |c: &DmshConfig| r.set_visible(c.is_custom(s));
    let snd = sender.clone();
    reset.connect_clicked(move |_| snd.input(Input::Reset(index)));
    let set = {
        let (snd, quiet) = (sender.clone(), quiet.clone());
        move |value: Value| {
            if !quiet.get() {
                snd.input(Input::Set(index, value));
            }
        }
    };

    match s.kind {
        Kind::Color => {
            let row = adw::ActionRow::builder().title(s.title).subtitle(s.subtitle).build();
            let dialog = gtk::ColorDialog::builder().with_alpha(true).title(s.title).build();
            let button = gtk::ColorDialogButton::builder().dialog(&dialog).valign(gtk::Align::Center).build();
            row.add_suffix(&reset);
            row.add_suffix(&button);
            button.connect_rgba_notify(move |b| set(Value::Str(hex(&b.rgba()))));
            bindings.add(move |c| {
                if let Some(color) = c.value(s).and_then(|v| v.as_str().and_then(|v| gdk::RGBA::parse(v).ok())) {
                    button.set_rgba(&color);
                }
                shows_reset(c);
            });
            row.upcast()
        }
        Kind::Font => {
            let row = adw::ActionRow::builder().title(s.title).subtitle(s.subtitle).build();
            let dialog = gtk::FontDialog::builder().title("Island Font").build();
            let button = gtk::FontDialogButton::builder()
                .dialog(&dialog)
                .level(gtk::FontLevel::Family)
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&reset);
            row.add_suffix(&button);
            button.connect_font_desc_notify(move |b| {
                if let Some(family) = b.font_desc().and_then(|d| d.family()) {
                    set(Value::Str(family.to_string()));
                }
            });
            bindings.add(move |c| {
                let family = c.get(s).and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "Inter".into());
                button.set_font_desc(&pango::FontDescription::from_string(&family));
                shows_reset(c);
            });
            row.upcast()
        }
        Kind::Text => {
            let row = adw::EntryRow::builder().title(s.title).show_apply_button(true).build();
            row.add_suffix(&reset);
            row.connect_apply(move |r| set(Value::Str(r.text().to_string())));
            let r = row.clone();
            bindings.add(move |c| {
                r.set_text(c.value(s).as_ref().and_then(Value::as_str).unwrap_or_default());
                shows_reset(c);
            });
            row.upcast()
        }
        Kind::Number { min, max, step, digits } => {
            let row = spin_row(s.title, s.subtitle, (min, max, step), digits);
            row.add_suffix(&reset);
            row.connect_value_notify(move |r| set(Value::Float(r.value())));
            let r = row.clone();
            bindings.add(move |c| {
                if let Some(v) = c.value(s).as_ref().and_then(Value::as_f64) {
                    r.set_value(v);
                }
                shows_reset(c);
            });
            row.upcast()
        }
        Kind::Duration { max } => {
            let subtitle =
                if s.subtitle.is_empty() { "Seconds".to_string() } else { format!("{} (seconds)", s.subtitle) };
            let row = spin_row(s.title, &subtitle, (0.0, max, 0.1), 1);
            row.add_suffix(&reset);
            row.connect_value_notify(move |r| set(Value::Str(dmsh::format_duration(r.value()))));
            let r = row.clone();
            bindings.add(move |c| {
                if let Some(seconds) = c.value(s).as_ref().and_then(Value::as_str).and_then(dmsh::parse_duration) {
                    r.set_value(seconds);
                }
                shows_reset(c);
            });
            row.upcast()
        }
        Kind::Switch => {
            let row = switch_row(s.title, s.subtitle);
            row.connect_active_notify(move |r| set(Value::Bool(r.is_active())));
            let r = row.clone();
            bindings.add(move |c| r.set_active(c.value(s).and_then(|v| v.as_bool()).unwrap_or(true)));
            row.upcast()
        }
    }
}

fn animations_row(sender: &ComponentSender<ShellPage>, bindings: &mut Bindings<DmshConfig>) -> adw::SwitchRow {
    let row = switch_row("Animations", "Spring animations; off snaps to the final state");
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_active_notify(move |r| {
        if !quiet.get() {
            s.input(Input::Animations(r.is_active()));
        }
    });
    let r = row.clone();
    bindings.add(move |c| r.set_active(c.animations()));
    row
}

/// A switch per connected display: whether it shows the islands.
fn displays_group(sender: &ComponentSender<ShellPage>, bindings: &mut Bindings<DmshConfig>) -> adw::PreferencesGroup {
    let group =
        adw::PreferencesGroup::builder().title("Displays").description("Which displays show the islands").build();
    let outputs = niri::outputs();
    if outputs.is_empty() {
        group.add(&adw::ActionRow::builder().title("No displays found").subtitle("niri didn't list any").build());
        return group;
    }
    let rows: Rc<RefCell<Vec<(String, adw::SwitchRow)>>> = Rc::default();
    let quiet: Rc<Cell<bool>> = bindings.quiet();
    for (name, description) in outputs {
        let row = switch_row(&name, &description);
        let (s, rows_ref, quiet) = (sender.clone(), rows.clone(), quiet.clone());
        row.connect_active_notify(move |_| {
            if quiet.get() {
                return;
            }
            let hidden = rows_ref.borrow().iter().filter(|(_, r)| !r.is_active()).map(|(n, _)| n.clone()).collect();
            s.input(Input::HiddenOutputs(hidden));
        });
        group.add(&row);
        rows.borrow_mut().push((name, row));
    }
    bindings.add(move |c| {
        let (only, except) = c.outputs();
        for (name, row) in rows.borrow().iter() {
            row.set_active(!except.contains(name) && (only.is_empty() || only.contains(name)));
        }
    });
    group
}
