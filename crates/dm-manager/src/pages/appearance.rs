//! Appearance: Qt/KDE apps (Dolphin) and GTK apps, plus the icon theme and fonts they share.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use dmm_sys::appearance::{self, ColorScheme, Current, SchemeColors};
use dmm_sys::kdl_text::Value;
use dmm_sys::niri::{self, Area, NiriFile};
use relm4::adw::prelude::*;
use relm4::gtk::pango;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::{Bindings, Hero, combo_row, switch_row};

/// libadwaita's accent colors, to draw the swatches.
const ACCENT_COLORS: &[(&str, &str)] = &[
    ("blue", "#3584e4"),
    ("teal", "#2190a4"),
    ("green", "#3a944a"),
    ("yellow", "#c88800"),
    ("orange", "#ed5b00"),
    ("red", "#e62d42"),
    ("pink", "#d56199"),
    ("purple", "#9141ac"),
    ("slate", "#6f8396"),
];

/// Plasma's accent colors.
const KDE_ACCENTS: &[(&str, (u8, u8, u8))] = &[
    ("Blue", (0x3d, 0xae, 0xe9)),
    ("Teal", (0x00, 0xd3, 0xb8)),
    ("Green", (0x3d, 0xd4, 0x25)),
    ("Yellow", (0xe8, 0xcb, 0x2d)),
    ("Orange", (0xe9, 0x64, 0x3a)),
    ("Red", (0xe9, 0x3d, 0x58)),
    ("Pink", (0xe9, 0x3a, 0x9a)),
    ("Purple", (0xb8, 0x75, 0xdc)),
    ("Violet", (0x92, 0x6e, 0xe4)),
    ("Gray", (0x68, 0x6b, 0x6f)),
];

/// Icons shown on an icon theme's card.
const SAMPLE_ICONS: &[&str] = &["folder", "text-x-generic", "image-x-generic", "utilities-terminal"];

pub struct AppearancePage {
    current: Current,
    /// Accent colors found in the wallpaper.
    palette: Vec<(u8, u8, u8)>,
    schemes: Vec<ColorScheme>,
    busy: bool,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    ColorScheme(String),
    WidgetStyle(String),
    Kvantum(String),
    IconTheme(String),
    GtkTheme(String),
    GtkColorScheme(&'static str),
    Accent(&'static str),
    /// The Qt/KDE accent color; `None` for the color scheme's own.
    KdeAccent(Option<(u8, u8, u8)>),
    Font {
        monospace: bool,
        family: String,
        size: f64,
    },
    GtkCss(bool),
}

#[derive(Debug)]
pub enum Cmd {
    Applied(Result<String, String>),
    /// The wallpaper and the accent colors found in it.
    Palette(Option<PathBuf>, Vec<(u8, u8, u8)>),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    hero: Hero,
    wallpaper: WallpaperColors,
    bindings: Bindings<(Current, Vec<ColorScheme>)>,
}

impl Component for AppearancePage {
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
        let schemes = appearance::color_schemes();
        let mut bindings = Bindings::new();

        let hero = Hero::new("dmm-preferences-desktop-appearance-symbolic", "Appearance");
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);
        root.add(&hero_group);

        // Qt & KDE
        let group = adw::PreferencesGroup::builder()
            .title("Qt &amp; KDE Apps")
            .description("Dolphin, Kate, Ark and other Qt apps")
            .build();
        group.add(&scheme_carousel(&schemes, &sender, &mut bindings));
        let (accent_card, wallpaper) = kde_accent(&sender, &mut bindings);
        group.add(&accent_card);
        let list = boxed_list();
        let styles = appearance::widget_styles();
        list.append(&choice(
            "Widget Style",
            styles.iter().map(|s| (s.id.clone(), s.name.clone())).collect(),
            |c| c.widget_style.clone(),
            Input::WidgetStyle,
            &sender,
            &mut bindings,
        ));
        let kvantum = choice(
            "Kvantum Theme",
            appearance::kvantum_themes().into_iter().map(|t| (t.clone(), t)).collect(),
            |c| c.kvantum_theme.clone(),
            Input::Kvantum,
            &sender,
            &mut bindings,
        );
        kvantum.set_enable_search(true);
        let k = kvantum.clone();
        bindings.add(move |(c, _)| k.set_visible(c.widget_style.as_deref() == Some("kvantum")));
        list.append(&kvantum);
        group.add(&list);
        root.add(&group);

        // GTK
        let group = adw::PreferencesGroup::builder().title("GTK Apps").description("GTK and GNOME apps").build();
        let list = boxed_list();
        list.append(&gtk_style_row(&sender, &mut bindings));
        list.append(&accent_row(&sender, &mut bindings));
        list.append(&choice(
            "GTK 3 Theme",
            appearance::gtk_themes().into_iter().map(|t| (t.clone(), t)).collect(),
            |c| c.gtk_theme.clone(),
            Input::GtkTheme,
            &sender,
            &mut bindings,
        ));
        list.append(&gtk_css_row(&sender, &mut bindings));
        group.add(&list);
        root.add(&group);

        // Icons
        let group = adw::PreferencesGroup::builder().title("Icons").description("For both Qt and GTK apps").build();
        group.add(&icon_carousel(&sender, &mut bindings));
        root.add(&group);

        // Fonts
        let group = adw::PreferencesGroup::builder().title("Fonts").description("For both Qt and GTK apps").build();
        group.add(&font_row("Interface", false, &sender, &mut bindings));
        group.add(&font_row("Monospace", true, &sender, &mut bindings));
        root.add(&group);

        let model = Self { current: appearance::current(), schemes, busy: false, palette: Vec::new() };
        let mut widgets = Widgets { hero, wallpaper, bindings };
        read_palette(&sender);
        model.show(&mut widgets);
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, _: &Self::Root) {
        if let Input::Visible(visible) = input {
            if visible {
                self.current = appearance::current();
                self.show(w);
                // The wallpaper may have changed (the Wallpapers page, Ctrl+Alt+T).
                read_palette(&sender);
            }
            return;
        }
        self.busy = true;
        sender.oneshot_command(async move {
            let result = tokio::task::spawn_blocking(move || apply(input)).await.unwrap_or_else(|e| Err(e.to_string()));
            Cmd::Applied(result)
        });
    }

    fn update_cmd_with_view(&mut self, w: &mut Widgets, cmd: Cmd, sender: ComponentSender<Self>, _: &Self::Root) {
        let result = match cmd {
            Cmd::Applied(result) => result,
            Cmd::Palette(image, colors) => {
                if colors != self.palette || w.wallpaper.buttons.is_empty() != colors.is_empty() {
                    w.wallpaper.set(image.as_deref(), &colors, &sender);
                }
                self.palette = colors;
                self.show(w);
                return;
            }
        };
        self.busy = false;
        match result {
            Ok(message) if !message.is_empty() => toast(&sender, message),
            Ok(_) => {}
            Err(e) => toast(&sender, e),
        }
        self.current = appearance::current();
        self.show(w);
    }
}

impl AppearancePage {
    fn show(&self, w: &mut Widgets) {
        let c = &self.current;
        let scheme = match &c.color_scheme {
            Some(id) => self.schemes.iter().find(|s| s.id == *id).map_or(id.clone(), |s| s.name.clone()) + " colors",
            None => "Custom colors".into(),
        };
        let style = c.widget_style.as_ref().map(|s| format!("{s} style"));
        let icons = c.icon_theme.as_ref().map(|id| {
            let name = appearance::icon_themes().into_iter().find(|t| t.id == *id).map_or(id.clone(), |t| t.name);
            format!("{name} icons")
        });
        let parts: Vec<String> = [Some(scheme), style, icons].into_iter().flatten().collect();
        w.hero.set_subtitle(&parts.join(" · "));
        w.bindings.refresh(&(c.clone(), self.schemes.clone()));
        w.wallpaper.select(c.accent_color);
    }
}

/// Finds accent colors in the current wallpaper (off the UI thread: it decodes the image).
fn read_palette(sender: &ComponentSender<AppearancePage>) {
    sender.oneshot_command(async {
        let found = tokio::task::spawn_blocking(|| {
            let image = dmm_sys::wallpaper::current().into_iter().next()?;
            let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&image, 96, 96, true).ok()?;
            let bytes = pixbuf.read_pixel_bytes();
            let (stride, channels) = (pixbuf.rowstride() as usize, pixbuf.n_channels() as usize);
            let mut pixels = Vec::new();
            for y in 0..pixbuf.height() as usize {
                for x in 0..pixbuf.width() as usize {
                    let i = y * stride + x * channels;
                    if let Some(p) = bytes.get(i..i + 3) {
                        pixels.push((p[0], p[1], p[2]));
                    }
                }
            }
            Some((image, dmm_sys::palette::accents(&pixels, 6)))
        })
        .await
        .ok()
        .flatten();
        match found {
            Some((image, colors)) => Cmd::Palette(Some(image), colors),
            None => Cmd::Palette(None, Vec::new()),
        }
    });
}

/// The accent card's row of colors from the wallpaper.
struct WallpaperColors {
    row: gtk::Box,
    thumbnail: gtk::Picture,
    swatches: adw::WrapBox,
    /// The toggle group the swatches join (with the presets).
    group: gtk::ToggleButton,
    custom: gtk::ColorDialogButton,
    quiet: Rc<std::cell::Cell<bool>>,
    buttons: Vec<((u8, u8, u8), gtk::ToggleButton)>,
}

impl WallpaperColors {
    fn set(
        &mut self,
        image: Option<&std::path::Path>,
        colors: &[(u8, u8, u8)],
        sender: &ComponentSender<AppearancePage>,
    ) {
        for (_, button) in self.buttons.drain(..) {
            self.swatches.remove(&button);
        }
        for color in colors {
            let swatch = color_swatch(*color, &appearance::hex(*color));
            swatch.set_group(Some(&self.group));
            let (s, q, color) = (sender.clone(), self.quiet.clone(), *color);
            swatch.connect_toggled(move |b| {
                if b.is_active() && !q.get() {
                    s.input(Input::KdeAccent(Some(color)));
                }
            });
            self.swatches.append(&swatch);
            self.buttons.push((color, swatch));
        }
        if let Some(image) = image {
            self.thumbnail.set_filename(Some(image));
        }
        self.row.set_visible(!colors.is_empty());
    }

    /// Marks the wallpaper color that's the accent, if one is.
    fn select(&self, accent: Option<(u8, u8, u8)>) {
        self.quiet.set(true);
        let mut matched = false;
        for (color, button) in &self.buttons {
            let on = accent == Some(*color);
            button.set_active(on);
            matched |= on;
        }
        if matched {
            self.custom.remove_css_class("chosen");
        }
        self.quiet.set(false);
    }
}

fn toast(sender: &ComponentSender<AppearancePage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

/// Applies a change (off the UI thread: the tools take a moment). Returns a message to show.
fn apply(input: Input) -> Result<String, String> {
    match input {
        Input::Visible(_) => Ok(String::new()),
        Input::ColorScheme(id) => appearance::set_color_scheme(&id).map(|()| String::new()),
        Input::KdeAccent(color) => appearance::set_accent_color(color).map(|()| String::new()),
        Input::WidgetStyle(id) => {
            appearance::set_widget_style(&id)?;
            // QT_STYLE_OVERRIDE in niri's environment would keep newly started apps on the old style.
            let config = niri::effective();
            if config.environment.string("QT_STYLE_OVERRIDE").is_some() && config.included {
                let mut file = NiriFile::load()?;
                file.set(&config, Area::Environment, "QT_STYLE_OVERRIDE", Some(Value::Str(id)))?;
                file.save()?;
                return Ok("Apps opened from now on use the new style".into());
            }
            Ok("Open apps switch styles when they restart".into())
        }
        Input::Kvantum(theme) => {
            appearance::set_kvantum_theme(&theme).map(|()| "Qt apps pick up the Kvantum theme when they restart".into())
        }
        Input::IconTheme(id) => appearance::set_icon_theme(&id).map(|()| String::new()),
        Input::GtkTheme(id) => appearance::set_gtk_theme(&id).map(|()| String::new()),
        Input::GtkColorScheme(scheme) => appearance::set_gtk_color_scheme(scheme).map(|()| String::new()),
        Input::Accent(accent) => appearance::set_accent(accent).map(|()| String::new()),
        Input::Font { monospace, family, size } => {
            appearance::set_font(monospace, &family, size).map(|()| String::new())
        }
        Input::GtkCss(on) => appearance::set_gtk_css(on).map(|()| "Restart GTK apps to see the change".into()),
    }
}

type Shown = (Current, Vec<ColorScheme>);

fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).margin_top(12).build()
}

/// A combo row over `(id, label)` choices.
fn choice(
    title: &str,
    choices: Vec<(String, String)>,
    current: impl Fn(&Current) -> Option<String> + 'static,
    input: fn(String) -> Input,
    sender: &ComponentSender<AppearancePage>,
    bindings: &mut Bindings<Shown>,
) -> adw::ComboRow {
    let labels: Vec<&str> = choices.iter().map(|(_, l)| l.as_str()).collect();
    let row = combo_row(title, "", &labels);
    let choices = Rc::new(choices);
    let (s, quiet, ch) = (sender.clone(), bindings.quiet(), choices.clone());
    row.connect_selected_notify(move |r| {
        if !quiet.get()
            && let Some((id, _)) = ch.get(r.selected() as usize)
        {
            s.input(input(id.clone()));
        }
    });
    let r = row.clone();
    bindings.add(move |(c, _)| {
        let index = current(c).and_then(|id| choices.iter().position(|(i, _)| i.eq_ignore_ascii_case(&id)));
        r.set_selected(index.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
        r.set_subtitle(if index.is_none() { "Not set" } else { "" });
    });
    row
}

/// Each card's id and button.
type Cards = Rc<RefCell<Vec<(String, gtk::Button)>>>;

/// A horizontally scrolling row of cards; the current one is outlined.
fn carousel(
    cards: Vec<(String, gtk::Widget, String)>,
    on_pick: impl Fn(String) + 'static,
) -> (gtk::ScrolledWindow, Cards) {
    let strip = gtk::Box::builder().spacing(10).margin_top(2).margin_bottom(6).margin_start(2).margin_end(2).build();
    let buttons: Cards = Rc::default();
    let on_pick = Rc::new(on_pick);
    for (id, preview, name) in cards {
        let label = gtk::Label::builder().label(&name).ellipsize(pango::EllipsizeMode::End).max_width_chars(16).build();
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).build();
        content.append(&preview);
        content.append(&label);
        let button = gtk::Button::builder().child(&content).tooltip_text(&name).css_classes(["theme-card"]).build();
        let (pick, i) = (on_pick.clone(), id.clone());
        button.connect_clicked(move |_| pick(i.clone()));
        strip.append(&button);
        buttons.borrow_mut().push((id, button));
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&strip)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .css_classes(["carousel"])
        .build();
    (scroller, buttons)
}

/// Marks the card for `current` and scrolls the strip to it.
fn select_card(scroller: &gtk::ScrolledWindow, buttons: &[(String, gtk::Button)], current: Option<&str>) {
    for (id, button) in buttons {
        if Some(id.as_str()) != current {
            button.remove_css_class("selected");
            continue;
        }
        button.add_css_class("selected");
        // Once laid out: center the card in the strip.
        let (scroller, button) = (scroller.clone(), button.clone());
        gtk::glib::idle_add_local_once(move || {
            let Some(strip) = button.parent() else { return };
            let Some(point) = button.compute_point(&strip, &gtk::graphene::Point::new(0.0, 0.0)) else { return };
            let adj = scroller.hadjustment();
            let target = f64::from(point.x()) - (adj.page_size() - f64::from(button.width())) / 2.0;
            adj.set_value(target.clamp(adj.lower(), (adj.upper() - adj.page_size()).max(adj.lower())));
        });
    }
}

fn scheme_carousel(
    schemes: &[ColorScheme],
    sender: &ComponentSender<AppearancePage>,
    bindings: &mut Bindings<Shown>,
) -> gtk::Widget {
    let cards = schemes.iter().map(|s| (s.id.clone(), scheme_preview(s.colors).upcast(), s.name.clone())).collect();
    let s = sender.clone();
    let (scroller, buttons) = carousel(cards, move |id| s.input(Input::ColorScheme(id)));
    let note = gtk::Label::builder()
        .label("These colors come from another tool. Pick a scheme to replace them.")
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label", "caption"])
        .margin_top(4)
        .build();
    let n = note.clone();
    let sc = scroller.clone();
    bindings.add(move |(c, _)| {
        select_card(&sc, &buttons.borrow(), c.color_scheme.as_deref());
        n.set_visible(c.color_scheme.is_none());
    });
    let column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    column.append(&scroller);
    column.append(&note);
    column.upcast()
}

/// A miniature window in the scheme's colors: a sidebar, a view with a selected row and text.
fn scheme_preview(colors: SchemeColors) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder().content_width(136).content_height(84).build();
    area.set_draw_func(move |_, cr, width, height| {
        let (w, h) = (f64::from(width), f64::from(height));
        let rgb = |c: (u8, u8, u8)| (f64::from(c.0) / 255.0, f64::from(c.1) / 255.0, f64::from(c.2) / 255.0);
        let fill = |c: (u8, u8, u8), alpha: f64, x: f64, y: f64, w: f64, h: f64, r: f64| {
            let (red, green, blue) = rgb(c);
            cr.set_source_rgba(red, green, blue, alpha);
            rounded(cr, x, y, w, h, r);
            let _ = cr.fill();
        };
        fill(colors.window, 1.0, 0.0, 0.0, w, h, 10.0);
        // Sidebar: a few entries and a button.
        for (i, width) in [22.0, 18.0, 24.0].iter().enumerate() {
            fill(colors.window_text, 0.45, 8.0, 12.0 + i as f64 * 10.0, *width, 4.0, 2.0);
        }
        fill(colors.button, 1.0, 6.0, h - 22.0, 30.0, 14.0, 5.0);
        fill(colors.accent, 1.0, 9.0, h - 18.0, 6.0, 6.0, 3.0);
        // The view.
        let (vx, vy) = (42.0, 8.0);
        fill(colors.view, 1.0, vx, vy, w - vx - 8.0, h - 16.0, 6.0);
        fill(colors.selection, 1.0, vx + 4.0, vy + 6.0, w - vx - 16.0, 10.0, 3.0);
        for (i, width) in [52.0, 38.0, 60.0, 30.0].iter().enumerate() {
            fill(colors.view_text, 0.5, vx + 8.0, vy + 24.0 + i as f64 * 10.0, *width, 4.0, 2.0);
        }
    });
    area
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn icon_carousel(sender: &ComponentSender<AppearancePage>, bindings: &mut Bindings<Shown>) -> gtk::ScrolledWindow {
    let display = gtk::gdk::Display::default().expect("a display");
    let cards = appearance::icon_themes()
        .into_iter()
        .map(|theme| {
            let lookup = gtk::IconTheme::new();
            lookup.set_theme_name(Some(&theme.id));
            // Search the same places as the display's theme (user and system icon dirs).
            lookup.set_search_path(
                &gtk::IconTheme::for_display(&display).search_path().iter().map(|p| p.as_path()).collect::<Vec<_>>(),
            );
            let row = gtk::Box::builder().spacing(6).halign(gtk::Align::Center).css_classes(["icon-preview"]).build();
            for name in SAMPLE_ICONS {
                let paintable =
                    lookup.lookup_icon(name, &[], 32, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
                let image = gtk::Image::from_paintable(Some(&paintable));
                image.set_pixel_size(28);
                row.append(&image);
            }
            (theme.id, row.upcast(), theme.name)
        })
        .collect();
    let s = sender.clone();
    let (scroller, buttons) = carousel(cards, move |id| s.input(Input::IconTheme(id)));
    let sc = scroller.clone();
    bindings.add(move |(c, _)| select_card(&sc, &buttons.borrow(), c.icon_theme.as_deref()));
    scroller
}

fn gtk_style_row(sender: &ComponentSender<AppearancePage>, bindings: &mut Bindings<Shown>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title("Style").subtitle("What GTK 4 and libadwaita apps prefer").build();
    let group = adw::ToggleGroup::builder().valign(gtk::Align::Center).build();
    for (name, label) in [("default", "Default"), ("prefer-light", "Light"), ("prefer-dark", "Dark")] {
        group.add(adw::Toggle::builder().name(name).label(label).build());
    }
    row.add_suffix(&group);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    group.connect_active_name_notify(move |g| {
        if quiet.get() {
            return;
        }
        let scheme = match g.active_name().as_deref() {
            Some("prefer-light") => "prefer-light",
            Some("prefer-dark") => "prefer-dark",
            _ => "default",
        };
        s.input(Input::GtkColorScheme(scheme));
    });
    bindings.add(move |(c, _)| group.set_active_name(Some(c.gtk_color_scheme.as_deref().unwrap_or("default"))));
    row
}

/// A round swatch painted in `color`, with a check mark when chosen.
fn color_swatch(color: (u8, u8, u8), tooltip: &str) -> gtk::ToggleButton {
    let area = gtk::DrawingArea::builder().content_width(24).content_height(24).build();
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        cr.arc(w / 2.0, h / 2.0, w.min(h) / 2.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgb(f64::from(color.0) / 255.0, f64::from(color.1) / 255.0, f64::from(color.2) / 255.0);
        let _ = cr.fill();
    });
    gtk::ToggleButton::builder().child(&area).tooltip_text(tooltip).css_classes(["swatch", "small", "painted"]).build()
}

/// The Qt/KDE accent: a preview of the icon theme's folder in it, and the choices.
fn kde_accent(sender: &ComponentSender<AppearancePage>, bindings: &mut Bindings<Shown>) -> (gtk::Box, WallpaperColors) {
    let preview = gtk::Image::builder().pixel_size(56).valign(gtk::Align::Center).build();
    let title = gtk::Label::builder().label("Accent Color").xalign(0.0).css_classes(["heading"]).build();
    let subtitle = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build();

    let swatches = adw::WrapBox::builder().child_spacing(6).line_spacing(6).margin_top(6).build();
    let scheme = gtk::ToggleButton::builder()
        .label("Scheme")
        .tooltip_text("The color scheme's own accent")
        .css_classes(["pill", "small-pill"])
        .build();
    let quiet = bindings.quiet();
    let s = sender.clone();
    let q = quiet.clone();
    scheme.connect_toggled(move |b| {
        if b.is_active() && !q.get() {
            s.input(Input::KdeAccent(None));
        }
    });
    scheme.set_valign(gtk::Align::Center);
    swatches.append(&scheme);
    let mut presets = Vec::new();
    for (name, color) in KDE_ACCENTS {
        let swatch = color_swatch(*color, name);
        swatch.set_group(Some(&scheme));
        let (s, q, color) = (sender.clone(), quiet.clone(), *color);
        swatch.connect_toggled(move |b| {
            if b.is_active() && !q.get() {
                s.input(Input::KdeAccent(Some(color)));
            }
        });
        swatches.append(&swatch);
        presets.push((color, swatch));
    }
    let custom = gtk::ColorDialogButton::builder()
        .dialog(&gtk::ColorDialog::builder().title("Accent Color").with_alpha(false).build())
        .tooltip_text("Another color")
        .valign(gtk::Align::Center)
        .build();
    let (s, q) = (sender.clone(), quiet.clone());
    custom.connect_rgba_notify(move |b| {
        if q.get() {
            return;
        }
        let c = b.rgba();
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        s.input(Input::KdeAccent(Some((byte(c.red()), byte(c.green()), byte(c.blue())))));
    });
    swatches.append(&custom);

    // Colors from the wallpaper, filled in once they're found.
    let thumbnail = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .width_request(46)
        .height_request(28)
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["wallpaper-thumb"])
        .build();
    let wall_swatches = adw::WrapBox::builder().child_spacing(6).line_spacing(6).build();
    let wall_label =
        gtk::Label::builder().label("From your wallpaper").xalign(0.0).css_classes(["dim-label", "caption"]).build();
    let wall_column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4).build();
    wall_column.append(&wall_label);
    wall_column.append(&wall_swatches);
    let wall_row = gtk::Box::builder().spacing(10).margin_top(10).visible(false).build();
    wall_row.append(&thumbnail);
    wall_row.append(&wall_column);
    let wallpaper = WallpaperColors {
        row: wall_row.clone(),
        thumbnail,
        swatches: wall_swatches,
        group: scheme.clone(),
        custom: custom.clone(),
        quiet: quiet.clone(),
        buttons: Vec::new(),
    };

    let text = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).hexpand(true).build();
    text.append(&title);
    text.append(&subtitle);
    text.append(&swatches);
    text.append(&wall_row);
    let card = gtk::Box::builder().spacing(16).css_classes(["card", "accent-card"]).margin_top(12).build();
    card.append(&preview);
    card.append(&text);

    bindings.add(move |(c, _)| {
        let shown = c.accent_color.or(c.palette_accent).unwrap_or((0x3d, 0xae, 0xe9));
        scheme.set_active(c.accent_color.is_none());
        for (color, swatch) in &presets {
            swatch.set_active(c.accent_color == Some(*color));
        }
        let custom_color = c.accent_color.is_some_and(|a| !presets.iter().any(|(p, _)| *p == a));
        custom.set_rgba(&gtk::gdk::RGBA::new(
            f32::from(shown.0) / 255.0,
            f32::from(shown.1) / 255.0,
            f32::from(shown.2) / 255.0,
            1.0,
        ));
        if custom_color {
            custom.add_css_class("chosen");
        } else {
            custom.remove_css_class("chosen");
        }
        // The folder icon, tinted, when the icon theme takes the accent (Breeze).
        let svg = c.icon_theme.as_deref().and_then(appearance::accent_folder_svg);
        let texture = svg.and_then(|svg| {
            let tinted = appearance::tint_svg(&svg, shown);
            gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_owned(tinted.into_bytes())).ok()
        });
        preview.set_visible(texture.is_some());
        preview.set_paintable(texture.as_ref());
        subtitle.set_label(if texture.is_some() {
            "Folders, selections and highlights in Qt and KDE apps"
        } else {
            "Selections and highlights in Qt and KDE apps (Breeze icons also use it for folders)"
        });
    });
    (card, wallpaper)
}

fn accent_row(sender: &ComponentSender<AppearancePage>, bindings: &mut Bindings<Shown>) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title("Accent Color").build();
    let swatches = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
    let mut first: Option<gtk::ToggleButton> = None;
    let mut buttons = Vec::new();
    for (name, _) in ACCENT_COLORS {
        let button = gtk::ToggleButton::builder()
            .child(&gtk::Image::from_icon_name("dmm-object-select-symbolic"))
            .tooltip_text(*name)
            .css_classes(["swatch", "small", &format!("gnome-accent-{name}")])
            .build();
        button.set_group(first.as_ref());
        let (s, quiet) = (sender.clone(), bindings.quiet());
        button.connect_toggled(move |b| {
            if b.is_active() && !quiet.get() {
                s.input(Input::Accent(name));
            }
        });
        swatches.append(&button);
        first.get_or_insert(button.clone());
        buttons.push((*name, button));
    }
    row.add_suffix(&swatches);
    bindings.add(move |(c, _)| {
        for (name, button) in &buttons {
            button.set_active(c.accent.as_deref() == Some(*name));
        }
    });
    row
}

fn gtk_css_row(sender: &ComponentSender<AppearancePage>, bindings: &mut Bindings<Shown>) -> adw::SwitchRow {
    let row = switch_row(
        "Colors From gtk.css",
        "Left by another theming tool; recolors every GTK app over the theme and accent",
    );
    let (s, quiet) = (sender.clone(), bindings.quiet());
    row.connect_active_notify(move |r| {
        if !quiet.get() {
            s.input(Input::GtkCss(r.is_active()));
        }
    });
    let r = row.clone();
    bindings.add(move |(c, _)| {
        r.set_visible(!c.gtk_css.is_empty() || !c.gtk_css_off.is_empty());
        r.set_active(!c.gtk_css.is_empty());
    });
    row
}

fn font_row(
    title: &str,
    monospace: bool,
    sender: &ComponentSender<AppearancePage>,
    bindings: &mut Bindings<Shown>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    let dialog = gtk::FontDialog::builder().title(format!("{title} Font")).build();
    if monospace {
        dialog.set_filter(Some(&gtk::CustomFilter::new(|item| {
            item.downcast_ref::<pango::FontFamily>().is_none_or(|f| f.is_monospace())
        })));
    }
    let button =
        gtk::FontDialogButton::builder().dialog(&dialog).level(gtk::FontLevel::Font).valign(gtk::Align::Center).build();
    row.add_suffix(&button);
    let (s, quiet) = (sender.clone(), bindings.quiet());
    button.connect_font_desc_notify(move |b| {
        if quiet.get() {
            return;
        }
        let Some(desc) = b.font_desc() else { return };
        let Some(family) = desc.family() else { return };
        let size = f64::from(desc.size()) / f64::from(pango::SCALE);
        s.input(Input::Font { monospace, family: family.to_string(), size: if size > 0.0 { size } else { 11.0 } });
    });
    bindings.add(move |(c, _)| {
        let font = if monospace { &c.monospace_font } else { &c.font };
        if let Some((family, size)) = font {
            button.set_font_desc(&pango::FontDescription::from_string(&format!("{family} {size}")));
        }
    });
    row
}
