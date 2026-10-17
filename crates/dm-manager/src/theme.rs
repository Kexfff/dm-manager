//! Look and feel: the dmsh stylesheet, our icons, and the user's accent color.

use std::cell::OnceCell;
use std::path::PathBuf;

use relm4::adw;
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

fn settings_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    config.join("dm-manager").join("settings.kdl")
}

fn read_settings() -> kdl::KdlDocument {
    std::fs::read_to_string(settings_path()).ok().and_then(|text| text.parse().ok()).unwrap_or_default()
}

pub fn saved_accent() -> &'static Accent {
    let doc = read_settings();
    let id = doc.get_arg("accent").and_then(|v| v.as_string()).unwrap_or("blue");
    ACCENTS.iter().find(|a| a.id == id).unwrap_or(&ACCENTS[0])
}

/// Saves the accent, keeping whatever else is in the settings file.
pub fn save_accent(accent: &Accent) {
    let mut doc = read_settings();
    doc.nodes_mut().retain(|n| n.name().value() != "accent");
    let mut node = kdl::KdlNode::new("accent");
    node.push(kdl::KdlEntry::new(accent.id));
    doc.nodes_mut().push(node);
    doc.autoformat();

    let path = settings_path();
    let written =
        path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, doc.to_string()));
    if let Err(e) = written {
        log::warn!("can't save {}: {e}", path.display());
    }
}
