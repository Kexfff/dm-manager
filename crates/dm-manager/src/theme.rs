//! Look and feel: the dmsh stylesheet, our icons, and the user's accent color.

use std::cell::OnceCell;

use relm4::adw;

use crate::settings;
use relm4::gtk::{self, gdk};

const RESOURCE_PREFIX: &str = "/io/github/kexfff/DmManager";

pub struct Accent {
    pub id: &'static str,
    pub name: &'static str,
    pub color: &'static str,
    /// Text and icons on top of the accent.
    pub fg: &'static str,
}

/// Apple's dark-mode system colors, to sit with dmsh's palette.
pub const ACCENTS: &[Accent] = &[
    Accent { id: "blue", name: "Blue", color: "#0a84ff", fg: "#ffffff" },
    Accent { id: "purple", name: "Purple", color: "#bf5af2", fg: "#ffffff" },
    Accent { id: "pink", name: "Pink", color: "#ff375f", fg: "#ffffff" },
    Accent { id: "red", name: "Red", color: "#ff453a", fg: "#ffffff" },
    Accent { id: "orange", name: "Orange", color: "#ff9f0a", fg: "#1c1c1e" },
    Accent { id: "yellow", name: "Yellow", color: "#ffd60a", fg: "#1c1c1e" },
    Accent { id: "green", name: "Green", color: "#30d158", fg: "#1c1c1e" },
    Accent { id: "teal", name: "Teal", color: "#40c8e0", fg: "#1c1c1e" },
    Accent { id: "graphite", name: "Graphite", color: "#98989d", fg: "#1c1c1e" },
];

thread_local! {
    static ACCENT_CSS: OnceCell<gtk::CssProvider> = const { OnceCell::new() };
}

/// Loads the stylesheet and icons, forces the dark scheme and applies the saved accent.
pub fn init() {
    let display = gdk::Display::default().expect("no display");
    gtk::IconTheme::for_display(&display).add_resource_path(&format!("{RESOURCE_PREFIX}/icons"));
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let base = gtk::CssProvider::new();
    base.load_from_resource(&format!("{RESOURCE_PREFIX}/style.css"));
    add_provider(&display, &base);

    let swatches = gtk::CssProvider::new();
    let css: String =
        ACCENTS.iter().map(|a| format!(".swatch-{} {{ background: {}; color: {}; }}\n", a.id, a.color, a.fg)).collect();
    swatches.load_from_string(&css);
    add_provider(&display, &swatches);

    ACCENT_CSS.with(|cell| add_provider(&display, cell.get_or_init(gtk::CssProvider::new)));
    apply_accent(saved_accent());
}

/// Above the user's `~/.config/gtk-4.0/gtk.css` (USER priority): theming tools write colors
/// there for all apps, and dm-manager keeps its own look and accent.
const PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 1;

fn add_provider(display: &gdk::Display, provider: &gtk::CssProvider) {
    gtk::style_context_add_provider_for_display(display, provider, PRIORITY);
}

/// Recolors the whole app. libadwaita derives the accent text color from the background.
pub fn apply_accent(accent: &Accent) {
    let css = format!(":root {{ --accent-bg-color: {}; --accent-fg-color: {}; }}", accent.color, accent.fg);
    ACCENT_CSS.with(|cell| cell.get_or_init(gtk::CssProvider::new).load_from_string(&css));
}

pub fn accent_index(accent: &Accent) -> usize {
    ACCENTS.iter().position(|a| a.id == accent.id).unwrap_or(0)
}

pub fn saved_accent() -> &'static Accent {
    let id = settings::get("accent").unwrap_or_else(|| "blue".into());
    ACCENTS.iter().find(|a| a.id == id).unwrap_or(&ACCENTS[0])
}

pub fn save_accent(accent: &Accent) {
    settings::set("accent", Some(accent.id));
}
