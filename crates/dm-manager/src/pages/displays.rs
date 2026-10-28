//! Displays: arrange monitors and set their mode, scale, rotation and variable refresh rate.
//!
//! Changes are tried live (`niri msg output`), then kept, which writes them to
//! dm-manager.kdl, or reverted, which happens on its own after a countdown. An output
//! configured in the user's own file is moved to dm-manager.kdl first, on request: niri only
//! reads the first block for an output.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use dmm_sys::niri::{self, InputConfig, NiriFile};
use dmm_sys::outputs::{self, Mode, Output as Display, Settings, Transform};
use relm4::adw::prelude::*;
use relm4::gtk::{glib, pango};
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::{Bindings, Hero, combo_row, switch_row};

const KEEP_SECONDS: u32 = 15;

pub struct DisplaysPage {
    outputs: Vec<Display>,
    config: InputConfig,
    /// What niri has now.
    applied: HashMap<String, Settings>,
    /// What the page shows (applied plus edits).
    pending: HashMap<String, Settings>,
    selected: String,
    error: Option<String>,
    confirm: Option<Confirm>,
}

/// The "Keep these settings?" question after applying.
struct Confirm {
    dialog: adw::AlertDialog,
    previous: HashMap<String, Settings>,
    left: u32,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Select(String),
    Change(String, Settings),
    Apply,
    Reset,
    Tick,
    Keep,
    Revert,
    Manage(String),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    apply: gtk::Button,
    reset: gtk::Button,
    canvas: Canvas,
    status: adw::StatusPage,
    groups: Vec<adw::PreferencesGroup>,
    detail: adw::PreferencesGroup,
    manage: gtk::Box,
    manage_label: gtk::Label,
    manage_button: gtk::Button,
    controls: gtk::ListBox,
    bindings: Bindings<Shown>,
}

/// What the controls show: the selected output, its settings, and whether it can be edited.
#[derive(Clone)]
struct Shown {
    output: Option<Display>,
    settings: Option<Settings>,
    editable: bool,
    last_on: bool,
}

impl Component for DisplaysPage {
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
        let hero = Hero::new("dmm-preferences-desktop-display-symbolic", "Displays");
        let reset = gtk::Button::builder().label("Reset").css_classes(["pill", "small-pill"]).visible(false).build();
        let apply = gtk::Button::builder()
            .label("Apply")
            .css_classes(["pill", "small-pill", "suggested-action"])
            .visible(false)
            .build();
        let s = sender.clone();
        reset.connect_clicked(move |_| s.input(Input::Reset));
        let s = sender.clone();
        apply.connect_clicked(move |_| s.input(Input::Apply));
        hero.add_suffix(&reset);
        hero.add_suffix(&apply);
        let group = adw::PreferencesGroup::new();
        group.add(&hero.root);
        root.add(&group);

        let status = adw::StatusPage::builder().icon_name("dmm-preferences-desktop-display-symbolic").build();
        status.add_css_class("compact");
        let status_group = adw::PreferencesGroup::new();
        status_group.add(&status);
        root.add(&status_group);

        let canvas = Canvas::new(&sender);
        let arrange = adw::PreferencesGroup::builder()
            .title("Arrangement")
            .description("Drag the displays to match how they stand on your desk")
            .build();
        arrange.add(&canvas.frame);
        root.add(&arrange);

        // The selected display.
        let manage_label = gtk::Label::builder().xalign(0.0).wrap(true).build();
        let manage_button = gtk::Button::builder()
            .label("Manage Here")
            .halign(gtk::Align::Start)
            .css_classes(["pill", "suggested-action"])
            .build();
        let manage = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .css_classes(["card", "notice"])
            .margin_bottom(12)
            .visible(false)
            .build();
        manage.append(&manage_label);
        manage.append(&manage_button);
        let mut bindings = Bindings::new();
        let controls = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        build_controls(&controls, &sender, &mut bindings);
        let detail = adw::PreferencesGroup::new();
        detail.add(&manage);
        detail.add(&controls);
        root.add(&detail);
        let s = sender.clone();
        let label = manage_label.clone();
        let button = manage_button.clone();
        button.connect_clicked(move |_| {
            if let Some(name) = label.widget_name().strip_prefix("output:") {
                s.input(Input::Manage(name.to_string()));
            }
        });

        let mut model = Self {
            outputs: Vec::new(),
            config: InputConfig::default(),
            applied: HashMap::new(),
            pending: HashMap::new(),
            selected: String::new(),
            error: None,
            confirm: None,
        };
        let mut widgets = Widgets {
            hero,
            apply,
            reset,
            canvas,
            status,
            groups: vec![arrange, detail],
            detail: adw::PreferencesGroup::new(),
            manage,
            manage_label,
            manage_button,
            controls,
            bindings,
        };
        widgets.detail = widgets.groups[1].clone();
        model.reload();
        model.show(&mut widgets);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => {
                if self.confirm.is_none() && self.pending == self.applied {
                    self.reload();
                }
            }
            Input::Visible(false) => return,
            Input::Select(name) => self.selected = name,
            Input::Change(name, settings) => {
                self.selected = name.clone();
                // Can't keep it there (see the notice); the canvas snaps back.
                if !self.config.included || self.configured_elsewhere(&name).is_some() {
                    self.show(w);
                    return;
                }
                self.pending.insert(name, settings);
            }
            Input::Reset => self.pending = self.applied.clone(),
            Input::Apply => self.apply(root, &sender),
            Input::Tick => {
                let Some(confirm) = &mut self.confirm else { return };
                confirm.left = confirm.left.saturating_sub(1);
                if confirm.left == 0 {
                    confirm.dialog.close();
                    sender.input(Input::Revert);
                    return;
                }
                confirm.dialog.set_body(&countdown(confirm.left));
                let s = sender.clone();
                glib::timeout_add_local_once(Duration::from_secs(1), move || s.input(Input::Tick));
                return;
            }
            Input::Keep => {
                let Some(_) = self.confirm.take() else { return };
                let changed: Vec<(String, Settings)> = self
                    .pending
                    .iter()
                    .filter(|(name, s)| self.applied.get(*name) != Some(*s))
                    .map(|(n, s)| (n.clone(), s.clone()))
                    .collect();
                let result = NiriFile::load().and_then(|mut file| {
                    for (name, settings) in &changed {
                        file.set_output(name, settings)?;
                    }
                    file.save()
                });
                match result {
                    Ok(()) => {
                        self.applied = self.pending.clone();
                        toast(&sender, "Display settings saved".into());
                    }
                    Err(e) => toast(&sender, format!("Applied, but not saved: {e}")),
                }
                // niri reloads the config; read back what it ended up with.
                let s = sender.clone();
                glib::timeout_add_local_once(Duration::from_millis(600), move || s.input(Input::Visible(true)));
            }
            Input::Revert => {
                let Some(confirm) = self.confirm.take() else { return };
                let mut errors = Vec::new();
                for (name, previous) in &confirm.previous {
                    if let Some(now) = self.pending.get(name)
                        && let Err(e) = outputs::apply(name, now, previous)
                    {
                        errors.push(e);
                    }
                }
                self.applied = confirm.previous.clone();
                self.pending = confirm.previous;
                toast(&sender, errors.first().cloned().unwrap_or_else(|| "Display settings reverted".into()));
            }
            Input::Manage(name) => {
                match niri::manage_output(&self.config, &name) {
                    Ok(()) => toast(&sender, format!("{name} is now managed here")),
                    Err(e) => toast(&sender, e),
                }
                self.reload();
            }
        }
        self.show(w);
    }
}

impl DisplaysPage {
    fn reload(&mut self) {
        self.config = niri::effective();
        match outputs::read() {
            Ok(list) => {
                self.applied = list.iter().map(|o| (o.name.clone(), o.settings())).collect();
                self.pending = self.applied.clone();
                if !list.iter().any(|o| o.name == self.selected) {
                    self.selected = list.first().map(|o| o.name.clone()).unwrap_or_default();
                }
                self.outputs = list;
                self.error = None;
            }
            Err(e) => {
                self.outputs.clear();
                self.error = Some(e);
            }
        }
    }

    /// The file that configures an output, when it's not dm-manager's (niri reads only that one).
    fn configured_elsewhere(&self, name: &str) -> Option<String> {
        self.config
            .outputs
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name) && !o.own)
            .map(|o| o.source.strip_prefix(niri::niri_dir()).unwrap_or(&o.source).display().to_string())
    }

    fn apply(&mut self, root: &adw::PreferencesPage, sender: &ComponentSender<Self>) {
        let previous = self.applied.clone();
        for output in &self.outputs {
            let (Some(from), Some(to)) = (self.applied.get(&output.name), self.pending.get(&output.name)) else {
                continue;
            };
            if from != to
                && let Err(e) = outputs::apply(&output.name, from, to)
            {
                toast(sender, e);
                // Undo what did apply.
                for (name, settings) in &previous {
                    if let Some(now) = self.pending.get(name) {
                        let _ = outputs::apply(name, now, settings);
                    }
                }
                self.pending = previous;
                return;
            }
        }
        self.applied = self.pending.clone();
        let dialog = adw::AlertDialog::new(Some("Keep These Display Settings?"), Some(&countdown(KEEP_SECONDS)));
        dialog.add_responses(&[("revert", "Revert"), ("keep", "Keep Changes")]);
        dialog.set_response_appearance("keep", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("keep"));
        dialog.set_close_response("revert");
        let s = sender.clone();
        dialog.connect_response(None, move |_, response| {
            s.input(if response == "keep" { Input::Keep } else { Input::Revert });
        });
        dialog.present(Some(root));
        self.confirm = Some(Confirm { dialog, previous, left: KEEP_SECONDS });
        let s = sender.clone();
        glib::timeout_add_local_once(Duration::from_secs(1), move || s.input(Input::Tick));
    }

    fn show(&self, w: &mut Widgets) {
        let changed = self.pending != self.applied && self.confirm.is_none();
        w.apply.set_visible(changed);
        w.reset.set_visible(changed);

        let on: Vec<&Display> =
            self.outputs.iter().filter(|o| self.pending.get(&o.name).is_some_and(|s| s.on)).collect();
        w.hero.set_subtitle(&match (&self.error, self.outputs.len()) {
            (Some(_), _) => "Needs niri".to_string(),
            (None, 0) => "No displays found".to_string(),
            (None, 1) => self.outputs[0].description(),
            (None, n) => format!("{n} displays, {} on", on.len()),
        });
        w.status.set_visible(self.error.is_some());
        if let Some(e) = &self.error {
            w.status.set_title("Can't Read the Displays");
            w.status.set_description(Some(e));
        }
        for group in &w.groups {
            group.set_visible(self.error.is_none() && !self.outputs.is_empty());
        }

        w.canvas.set(
            self.outputs
                .iter()
                .filter_map(|o| {
                    let s = self.pending.get(&o.name)?.clone();
                    s.on.then(|| (o.clone(), s))
                })
                .collect(),
            &self.selected,
        );

        let output = self.outputs.iter().find(|o| o.name == self.selected).cloned();
        let elsewhere = output.as_ref().and_then(|o| self.configured_elsewhere(&o.name));
        let blocked = match (&output, &elsewhere) {
            _ if !self.config.included => Some(
                "niri doesn't read DM Manager's file yet, so display settings can't be saved. Turn it on in \
                 Keyboard & Input."
                    .to_string(),
            ),
            (Some(o), Some(file)) => Some(format!(
                "{}'s settings are in {file}, and niri only reads the first settings for a display. To change it \
                 here, move them to DM Manager's file (the original is commented out, not deleted).",
                o.name
            )),
            _ => None,
        };
        w.manage.set_visible(blocked.is_some());
        w.manage_button.set_visible(elsewhere.is_some() && self.config.included);
        if let Some(text) = &blocked {
            w.manage_label.set_label(text);
        }
        if let Some(o) = &output {
            w.manage_label.set_widget_name(&format!("output:{}", o.name));
        }
        w.detail.set_title(&output.as_ref().map(|o| format!("{} ({})", o.description(), o.name)).unwrap_or_default());
        let shown = Shown {
            settings: output.as_ref().and_then(|o| self.pending.get(&o.name).cloned()),
            output,
            editable: blocked.is_none() && self.confirm.is_none(),
            last_on: on.len() <= 1,
        };
        w.controls.set_sensitive(shown.editable);
        w.bindings.refresh(&shown);
    }
}

fn toast(sender: &ComponentSender<DisplaysPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn countdown(seconds: u32) -> String {
    format!("The previous settings come back in {seconds} second{}.", if seconds == 1 { "" } else { "s" })
}

/// Distinct resolutions, largest first.
fn resolutions(output: &Display) -> Vec<(u32, u32)> {
    let mut list: Vec<(u32, u32)> = output.modes.iter().map(|m| (m.width, m.height)).collect();
    list.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)).then(b.0.cmp(&a.0)));
    list.dedup();
    list
}

/// The modes at a resolution, fastest first.
fn rates(output: &Display, (w, h): (u32, u32)) -> Vec<Mode> {
    let mut list: Vec<Mode> = output.modes.iter().filter(|m| (m.width, m.height) == (w, h)).copied().collect();
    list.sort_by_key(|m| std::cmp::Reverse(m.refresh));
    list.dedup_by_key(|m| m.refresh);
    list
}

/// The rows for the selected display. Each sends `Input::Change` with the whole settings.
fn build_controls(list: &gtk::ListBox, sender: &ComponentSender<DisplaysPage>, bindings: &mut Bindings<Shown>) {
    let current: Rc<RefCell<Option<(Display, Settings)>>> = Rc::default();
    let quiet = bindings.quiet();
    let change = {
        let (s, current, quiet) = (sender.clone(), current.clone(), quiet.clone());
        Rc::new(move |edit: &dyn Fn(&Display, &mut Settings)| {
            if quiet.get() {
                return;
            }
            let Some((output, mut settings)) = current.borrow().clone() else { return };
            edit(&output, &mut settings);
            s.input(Input::Change(output.name.clone(), settings));
        })
    };

    let enabled = switch_row("Use This Display", "");
    let c = change.clone();
    enabled.connect_active_notify(move |r| {
        let on = r.is_active();
        c(&|o, s| {
            s.on = on;
            if on && s.mode.is_none() {
                s.mode = o.preferred_mode();
            }
        })
    });
    list.append(&enabled);

    let resolution = combo_row("Resolution", "", &[]);
    let refresh = combo_row("Refresh Rate", "", &[]);
    let c = change.clone();
    resolution.connect_selected_notify(move |r| {
        let index = r.selected() as usize;
        c(&|o, s| {
            if let Some(res) = resolutions(o).get(index) {
                // Keep the rate if the new resolution has it, else the fastest.
                let rates = rates(o, *res);
                s.mode = s.mode.and_then(|m| rates.iter().find(|r| r.refresh == m.refresh).copied()).or(rates.first().copied());
            }
        })
    });
    let c = change.clone();
    refresh.connect_selected_notify(move |r| {
        let index = r.selected() as usize;
        c(&|o, s| {
            if let Some(m) = s.mode
                && let Some(rate) = rates(o, (m.width, m.height)).get(index)
            {
                s.mode = Some(*rate);
            }
        })
    });
    list.append(&resolution);
    list.append(&refresh);

    let scale = combo_row("Scale", "Bigger text and controls", &[]);
    let scale_values: Rc<RefCell<Vec<f64>>> = Rc::default();
    let (c, values) = (change.clone(), scale_values.clone());
    scale.connect_selected_notify(move |r| {
        if let Some(v) = values.borrow().get(r.selected() as usize).copied() {
            c(&|_, s| s.scale = v);
        }
    });
    list.append(&scale);

    let labels: Vec<&str> = Transform::ALL.iter().map(|t| t.label()).collect();
    let rotation = combo_row("Rotation", "", &labels);
    let c = change.clone();
    rotation.connect_selected_notify(move |r| {
        if let Some(t) = Transform::ALL.get(r.selected() as usize).copied() {
            c(&|_, s| s.transform = t);
        }
    });
    list.append(&rotation);

    let vrr = switch_row("Variable Refresh Rate", "Matches the refresh rate to games and video");
    let c = change.clone();
    vrr.connect_active_notify(move |r| {
        let on = r.is_active();
        c(&|_, s| s.vrr = on);
    });
    list.append(&vrr);

    bindings.add(move |shown: &Shown| {
        let (Some(output), Some(settings)) = (&shown.output, &shown.settings) else { return };
        *current.borrow_mut() = Some((output.clone(), settings.clone()));
        enabled.set_active(settings.on);
        // Turning off the last display would leave nothing to see.
        enabled.set_sensitive(!(settings.on && shown.last_on));
        let mode = settings.mode;

        let res = resolutions(output);
        let labels: Vec<String> = res.iter().map(|(w, h)| format!("{w} × {h}")).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        resolution.set_model(Some(&gtk::StringList::new(&labels)));
        let res_index = mode.and_then(|m| res.iter().position(|r| *r == (m.width, m.height)));
        resolution.set_selected(res_index.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));

        let rate_list = mode.map(|m| rates(output, (m.width, m.height))).unwrap_or_default();
        let labels: Vec<String> = rate_list.iter().map(|m| m.label().rsplit(" · ").next().unwrap_or("").to_string()).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        refresh.set_model(Some(&gtk::StringList::new(&labels)));
        let rate_index = mode.and_then(|m| rate_list.iter().position(|r| r.refresh == m.refresh));
        refresh.set_selected(rate_index.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));

        let values = outputs::scales(settings.scale);
        let labels: Vec<String> = values.iter().map(|v| format!("{:.0}%", v * 100.0)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        scale.set_model(Some(&gtk::StringList::new(&labels)));
        scale.set_selected(values.iter().position(|v| (v - settings.scale).abs() < 1e-6).unwrap_or(0) as u32);
        *scale_values.borrow_mut() = values;

        rotation.set_selected(Transform::ALL.iter().position(|t| *t == settings.transform).unwrap_or(0) as u32);
        vrr.set_visible(output.vrr_supported);
        vrr.set_active(settings.vrr);
        for row in [resolution.upcast_ref::<gtk::Widget>(), refresh.upcast_ref(), scale.upcast_ref(), rotation.upcast_ref()] {
            row.set_sensitive(settings.on);
        }
    });
}

/// The arrangement: monitors as tiles drawn to scale, which can be dragged (edges snap to each
/// other). Tiles are widgets placed by a custom layout, so their text is ordinary labels.
struct Canvas {
    frame: gtk::Box,
    area: gtk::Box,
    state: Rc<RefCell<CanvasState>>,
}

#[derive(Default)]
struct CanvasState {
    /// Each output and its settings (on ones only), and its tile.
    items: Vec<(Display, Settings)>,
    tiles: Vec<gtk::Box>,
    /// While dragging: the item and where it started (logical).
    drag: Option<(usize, f64, f64)>,
    /// Logical → widget: scale and offset, from the last layout.
    view: (f64, f64, f64),
}

impl Canvas {
    fn new(sender: &ComponentSender<DisplaysPage>) -> Self {
        let state: Rc<RefCell<CanvasState>> = Rc::default();
        let st = state.clone();
        let layout = CanvasLayout::new(move |width, height| place(&mut st.borrow_mut(), width, height));
        let area = gtk::Box::builder().hexpand(true).overflow(gtk::Overflow::Hidden).build();
        area.set_layout_manager(Some(layout));

        let drag = gtk::GestureDrag::new();
        let (st, s) = (state.clone(), sender.clone());
        drag.connect_drag_begin(move |_, x, y| {
            let mut state = st.borrow_mut();
            let (k, ox, oy) = state.view;
            let (lx, ly) = ((x - ox) / k, (y - oy) / k);
            let hit = state.items.iter().position(|(_, s)| {
                let (w, h) = s.logical_size();
                lx >= f64::from(s.x) && lx <= f64::from(s.x) + w && ly >= f64::from(s.y) && ly <= f64::from(s.y) + h
            });
            if let Some(i) = hit {
                let (o, set) = &state.items[i];
                let (name, x0, y0) = (o.name.clone(), f64::from(set.x), f64::from(set.y));
                state.drag = Some((i, x0, y0));
                for (j, tile) in state.tiles.iter().enumerate() {
                    set_class(tile, "selected", j == i);
                }
                s.input(Input::Select(name));
            }
        });
        let (st, a) = (state.clone(), area.clone());
        drag.connect_drag_update(move |_, dx, dy| {
            let mut state = st.borrow_mut();
            let Some((i, x0, y0)) = state.drag else { return };
            let k = state.view.0;
            let (x, y) = snap(&state.items, i, x0 + dx / k, y0 + dy / k, 14.0 / k);
            state.items[i].1.x = x;
            state.items[i].1.y = y;
            a.queue_allocate();
        });
        let (st, s, a) = (state.clone(), sender.clone(), area.clone());
        drag.connect_drag_end(move |_, _, _| {
            let mut state = st.borrow_mut();
            let Some((i, x0, y0)) = state.drag.take() else { return };
            let (o, settings) = &state.items[i];
            if (f64::from(settings.x), f64::from(settings.y)) != (x0, y0) {
                s.input(Input::Change(o.name.clone(), settings.clone()));
            }
            // Fit the new arrangement.
            a.queue_allocate();
        });
        area.add_controller(drag);

        let frame = gtk::Box::builder().css_classes(["card", "canvas-card"]).build();
        frame.append(&area);
        Self { frame, area, state }
    }

    fn set(&self, items: Vec<(Display, Settings)>, selected: &str) {
        let mut state = self.state.borrow_mut();
        if state.drag.is_some() {
            return;
        }
        for tile in state.tiles.drain(..) {
            self.area.remove(&tile);
        }
        for (output, settings) in &items {
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Fill)
                .css_classes(["display-tile"])
                .overflow(gtk::Overflow::Hidden)
                .build();
            set_class(&tile, "selected", output.name == selected);
            let name = gtk::Label::builder()
                .label(&output.name)
                .css_classes(["heading"])
                .ellipsize(pango::EllipsizeMode::End)
                .vexpand(true)
                .valign(gtk::Align::End)
                .build();
            let mode = gtk::Label::builder()
                .label(settings.mode.map(|m| format!("{} × {}", m.width, m.height)).unwrap_or_default())
                .css_classes(["caption", "dim-label"])
                .ellipsize(pango::EllipsizeMode::End)
                .vexpand(true)
                .valign(gtk::Align::Start)
                .build();
            tile.append(&name);
            tile.append(&mode);
            self.area.append(&tile);
            state.tiles.push(tile);
        }
        state.items = items;
        drop(state);
        self.area.queue_allocate();
    }
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// Fits the arrangement into the canvas (keeping the view while dragging, so it doesn't jump)
/// and gives each tile its rectangle.
fn place(state: &mut CanvasState, w: f64, h: f64) {
    if state.items.is_empty() {
        return;
    }
    if state.drag.is_none() || state.view.0 == 0.0 {
        let bounds = state.items.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, (_, s)| {
            let (sw, sh) = s.logical_size();
            let (x, y) = (f64::from(s.x), f64::from(s.y));
            (b.0.min(x), b.1.min(y), b.2.max(x + sw), b.3.max(y + sh))
        });
        let pad = 28.0;
        let (bw, bh) = ((bounds.2 - bounds.0).max(1.0), (bounds.3 - bounds.1).max(1.0));
        let k = ((w - 2.0 * pad) / bw).min((h - 2.0 * pad) / bh).clamp(0.001, 0.25);
        state.view = (k, (w - bw * k) / 2.0 - bounds.0 * k, (h - bh * k) / 2.0 - bounds.1 * k);
    }
    let (k, ox, oy) = state.view;
    for ((_, s), tile) in state.items.iter().zip(&state.tiles) {
        let (sw, sh) = s.logical_size();
        // GTK wants children measured before they're allocated; tiles never go below that.
        let (min_w, ..) = tile.measure(gtk::Orientation::Horizontal, -1);
        let (min_h, ..) = tile.measure(gtk::Orientation::Vertical, -1);
        let rect = gtk::Allocation::new(
            (ox + f64::from(s.x) * k + 2.0).round() as i32,
            (oy + f64::from(s.y) * k + 2.0).round() as i32,
            ((sw * k - 4.0).round() as i32).max(min_w),
            ((sh * k - 4.0).round() as i32).max(min_h),
        );
        tile.size_allocate(&rect, -1);
    }
}

/// Moves item `i` to (x, y), snapped to the other monitors' edges within `reach`.
fn snap(items: &[(Display, Settings)], i: usize, x: f64, y: f64, reach: f64) -> (i32, i32) {
    let (w, h) = items[i].1.logical_size();
    let (mut best_x, mut best_y) = ((x, reach), (y, reach));
    for (j, (_, other)) in items.iter().enumerate() {
        if j == i {
            continue;
        }
        let (ow, oh) = other.logical_size();
        let (ox, oy) = (f64::from(other.x), f64::from(other.y));
        for cx in [ox - w, ox + ow, ox, ox + ow - w] {
            if (cx - x).abs() < best_x.1 {
                best_x = (cx, (cx - x).abs());
            }
        }
        for cy in [oy - h, oy + oh, oy, oy + oh - h] {
            if (cy - y).abs() < best_y.1 {
                best_y = (cy, (cy - y).abs());
            }
        }
    }
    (best_x.0.round() as i32, best_y.0.round() as i32)
}

glib::wrapper! {
    /// Lays out the canvas's tiles with a closure: 260 px tall, as wide as given.
    pub struct CanvasLayout(ObjectSubclass<imp::CanvasLayout>) @extends gtk::LayoutManager;
}

impl CanvasLayout {
    fn new(allocate: impl Fn(f64, f64) + 'static) -> Self {
        let layout: Self = glib::Object::new();
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(&layout).allocate.replace(Some(Box::new(allocate)));
        layout
    }
}

mod imp {
    use std::cell::RefCell;

    use relm4::gtk::{self, glib, subclass::prelude::*};

    type Allocate = Box<dyn Fn(f64, f64)>;

    #[derive(Default)]
    pub struct CanvasLayout {
        pub allocate: RefCell<Option<Allocate>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CanvasLayout {
        const NAME: &'static str = "DmmCanvasLayout";
        type Type = super::CanvasLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for CanvasLayout {}

    impl LayoutManagerImpl for CanvasLayout {
        fn request_mode(&self, _: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, _: &gtk::Widget, orientation: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Vertical => (260, 260, -1, -1),
                _ => (0, 0, -1, -1),
            }
        }

        fn allocate(&self, _: &gtk::Widget, width: i32, height: i32, _: i32) {
            if let Some(allocate) = &*self.allocate.borrow() {
                allocate(f64::from(width), f64::from(height));
            }
        }
    }
}
