//! How apps look: Qt/KDE apps (Dolphin and friends, through the KDE platform theme's
//! `kdeglobals`, plus qt6ct and Kvantum), GTK apps (gsettings and `settings.ini`), and the
//! icon theme and fonts both share.
//!
//! Changes go through KDE's own tools (`plasma-apply-colorscheme`, `kwriteconfig6`) where they
//! exist, and running KDE apps are told to reload, so most changes show up immediately.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::ini::Ini;

pub type Rgb = (u8, u8, u8);

/// The colors a scheme preview needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchemeColors {
    pub window: Rgb,
    pub window_text: Rgb,
    pub view: Rgb,
    pub view_text: Rgb,
    pub button: Rgb,
    pub selection: Rgb,
    pub accent: Rgb,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorScheme {
    /// The file name without `.colors`: what KDE stores as the current scheme.
    pub id: String,
    pub name: String,
    pub colors: SchemeColors,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
}

/// What's set now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Current {
    /// `None` when kdeglobals' colors came from elsewhere (another theming tool).
    pub color_scheme: Option<String>,
    pub widget_style: Option<String>,
    pub kvantum_theme: Option<String>,
    /// The Qt/KDE accent set on top of the scheme; `None` uses the scheme's.
    pub accent_color: Option<Rgb>,
    /// The palette's accent now (the scheme's, or the one set on top).
    pub palette_accent: Option<Rgb>,
    pub icon_theme: Option<String>,
    pub gtk_theme: Option<String>,
    /// gsettings `color-scheme`: `default`, `prefer-dark` or `prefer-light`.
    pub gtk_color_scheme: Option<String>,
    pub accent: Option<String>,
    /// `(family, size in points)`
    pub font: Option<(String, f64)>,
    pub monospace_font: Option<(String, f64)>,
    /// `gtk.css` files (from theming tools) that recolor every GTK app.
    pub gtk_css: Vec<PathBuf>,
    /// The same files, switched off here.
    pub gtk_css_off: Vec<PathBuf>,
}

/// libadwaita's accent colors (gsettings names).
pub const ACCENTS: &[&str] = &["blue", "teal", "green", "yellow", "orange", "red", "pink", "purple", "slate"];

fn data_dirs() -> Vec<PathBuf> {
    let home = crate::home().unwrap_or_default();
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs = vec![data_home];
    dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    dirs
}

/// Entries of `<data dir>/<sub>` across all data dirs, earlier dirs first, unique by name.
fn entries(sub: &str, extra: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut roots: Vec<PathBuf> = extra.to_vec();
    roots.extend(data_dirs().into_iter().map(|d| d.join(sub)));
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for root in roots {
        let Ok(dir) = std::fs::read_dir(&root) else { continue };
        let mut names: Vec<_> = dir.flatten().collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, entry.path()));
            }
        }
    }
    out
}

pub fn color_schemes() -> Vec<ColorScheme> {
    let mut schemes: Vec<ColorScheme> = entries("color-schemes", &[])
        .into_iter()
        .filter_map(|(file, path)| {
            let id = file.strip_suffix(".colors")?.to_string();
            let ini = Ini::load(&path);
            let name = ini.get("General", "Name").filter(|n| !n.is_empty()).unwrap_or(&id).to_string();
            Some(ColorScheme { colors: scheme_colors(&ini), id, name })
        })
        .collect();
    schemes.sort_by_key(|s| s.name.to_lowercase());
    schemes
}

fn scheme_colors(ini: &Ini) -> SchemeColors {
    let color = |section: &str, key: &str| ini.get(section, key).and_then(parse_color).unwrap_or_default();
    SchemeColors {
        window: color("Colors:Window", "BackgroundNormal"),
        window_text: color("Colors:Window", "ForegroundNormal"),
        view: color("Colors:View", "BackgroundNormal"),
        view_text: color("Colors:View", "ForegroundNormal"),
        button: color("Colors:Button", "BackgroundNormal"),
        selection: color("Colors:Selection", "BackgroundNormal"),
        accent: color("Colors:View", "DecorationFocus"),
    }
}

/// `r,g,b` (KDE's usual form) or `#rrggbb`.
fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let n = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
        return Some(((n >> 16) as u8, (n >> 8) as u8, n as u8));
    }
    let mut parts = s.split(',').map(|p| p.trim().parse::<u8>());
    Some((parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?))
}

/// Qt widget styles that are installed: `(id, name)`, the id being what kdeglobals stores.
pub fn widget_styles() -> Vec<Theme> {
    let dirs: Vec<PathBuf> = [
        "/usr/lib/qt6/plugins/styles",
        "/usr/lib64/qt6/plugins/styles",
        "/usr/lib/x86_64-linux-gnu/qt6/plugins/styles",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    let has = |file: &str| dirs.iter().any(|d| d.join(file).exists());
    let mut styles = vec![];
    for (file, id, name) in [
        ("breeze6.so", "Breeze", "Breeze"),
        ("darkly6.so", "Darkly", "Darkly"),
        ("libkvantum.so", "kvantum", "Kvantum"),
        ("UnionWidgetsStyle.so", "Union", "Union"),
        ("lightly6.so", "Lightly", "Lightly"),
        ("klassik6.so", "Klassik", "Klassik"),
    ] {
        if has(file) {
            styles.push(Theme { id: id.into(), name: name.into() });
        }
    }
    // Built into Qt.
    styles.push(Theme { id: "Fusion".into(), name: "Fusion".into() });
    styles
}

pub fn kvantum_themes() -> Vec<String> {
    let user = crate::config_dir().join("Kvantum");
    let mut themes: Vec<String> =
        entries("Kvantum", &[user]).into_iter().filter(|(_, path)| path.is_dir()).map(|(name, _)| name).collect();
    themes.sort_by_key(|t| t.to_lowercase());
    themes
}

/// GTK 3 themes (with a `gtk-3.0` stylesheet; keybinding themes like Emacs have none).
pub fn gtk_themes() -> Vec<String> {
    let home = crate::home().unwrap_or_default();
    let mut themes: Vec<String> = entries("themes", &[home.join(".themes")])
        .into_iter()
        .filter(|(_, path)| path.join("gtk-3.0/gtk.css").exists() || path.join("gtk-3.0/gtk-dark.css").exists())
        .map(|(name, _)| name)
        .collect();
    if !themes.iter().any(|t| t == "Adwaita") {
        // Built into GTK.
        themes.push("Adwaita".into());
    }
    themes.sort_by_key(|t| t.to_lowercase());
    themes
}

/// Icon themes (not cursor-only themes, not the hicolor fallback).
pub fn icon_themes() -> Vec<Theme> {
    let home = crate::home().unwrap_or_default();
    let mut themes: Vec<Theme> = entries("icons", &[home.join(".icons")])
        .into_iter()
        .filter_map(|(id, path)| {
            let ini = Ini::load(&path.join("index.theme"));
            let has_icons = ini.get("Icon Theme", "Directories").is_some_and(|d| !d.trim().is_empty());
            let hidden = ini.get("Icon Theme", "Hidden").is_some_and(|h| h.eq_ignore_ascii_case("true"));
            // Adwaita hides itself from KDE's list, but it's a real choice.
            if !has_icons || id == "hicolor" || (hidden && id != "Adwaita") {
                return None;
            }
            let name = ini.get("Icon Theme", "Name").filter(|n| !n.is_empty()).unwrap_or(&id).to_string();
            Some(Theme { id, name })
        })
        .collect();
    themes.sort_by_key(|t| t.name.to_lowercase());
    themes
}

fn kdeglobals() -> PathBuf {
    crate::config_dir().join("kdeglobals")
}

fn qt6ct_conf() -> PathBuf {
    crate::config_dir().join("qt6ct").join("qt6ct.conf")
}

fn kvantum_conf() -> PathBuf {
    crate::config_dir().join("Kvantum").join("kvantum.kvconfig")
}

fn gtk_settings(version: &str) -> PathBuf {
    crate::config_dir().join(version).join("settings.ini")
}

const GTK_CSS_OFF: &str = "gtk.css.off-by-dm-manager";

pub fn current() -> Current {
    let kde = Ini::load(&kdeglobals());
    let font = |key| kde.get("General", key).and_then(parse_qt_font);
    let gtk_css_dirs = || ["gtk-3.0", "gtk-4.0"].into_iter().map(|v| crate::config_dir().join(v));
    Current {
        color_scheme: kde.get("General", "ColorScheme").map(String::from),
        widget_style: kde.get("KDE", "widgetStyle").map(String::from),
        kvantum_theme: Ini::load(&kvantum_conf()).get("General", "theme").map(String::from),
        accent_color: kde.get("General", "AccentColor").and_then(parse_color),
        palette_accent: kde.get("Colors:View", "DecorationFocus").and_then(parse_color),
        icon_theme: kde.get("Icons", "Theme").map(String::from).or_else(|| gsetting("icon-theme")),
        gtk_theme: gsetting("gtk-theme"),
        gtk_color_scheme: gsetting("color-scheme"),
        accent: gsetting("accent-color"),
        font: font("font").or_else(|| gsetting("font-name").and_then(|f| parse_gtk_font(&f))),
        monospace_font: font("fixed").or_else(|| gsetting("monospace-font-name").and_then(|f| parse_gtk_font(&f))),
        gtk_css: gtk_css_dirs().map(|d| d.join("gtk.css")).filter(|p| p.exists()).collect(),
        gtk_css_off: gtk_css_dirs().map(|d| d.join(GTK_CSS_OFF)).filter(|p| p.exists()).collect(),
    }
}

fn gsetting(key: &str) -> Option<String> {
    let out = Command::new("gsettings").args(["get", "org.gnome.desktop.interface", key]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string();
    (!value.is_empty()).then_some(value)
}

fn set_gsetting(key: &str, value: &str) {
    quiet(Command::new("gsettings").args(["set", "org.gnome.desktop.interface", key, value]));
}

fn quiet(cmd: &mut Command) -> bool {
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn kwrite(group: &str, key: &str, value: &str) -> Result<(), String> {
    let ok = quiet(Command::new("kwriteconfig6").args(["--file", "kdeglobals", "--group", group, "--key", key, value]));
    if ok {
        return Ok(());
    }
    // No KDE tools: edit the file directly.
    let mut ini = Ini::load(&kdeglobals());
    ini.set(group, key, value);
    ini.save(&kdeglobals())
}

/// Sets a key in GTK's settings.ini files that exist (creating none).
fn gtk_ini(key: &str, value: &str) {
    for version in ["gtk-3.0", "gtk-4.0"] {
        let path = gtk_settings(version);
        if path.exists() {
            let mut ini = Ini::load(&path);
            ini.set("Settings", key, value);
            let _ = ini.save(&path);
        }
    }
}

/// Sets a key in qt6ct's config, if qt6ct is set up.
fn qt6ct(key: &str, value: &str) {
    let path = qt6ct_conf();
    if path.exists() {
        let mut ini = Ini::load(&path);
        ini.set("Appearance", key, value);
        let _ = ini.save(&path);
    }
}

/// Tells running KDE apps that a setting changed (`KGlobalSettings::ChangeType`).
fn notify_kde(change: i32) {
    quiet(Command::new("dbus-send").args([
        "--session",
        "--type=signal",
        "/KGlobalSettings",
        "org.kde.KGlobalSettings.notifyChange",
        &format!("int32:{change}"),
        "int32:0",
    ]));
}

const PALETTE_CHANGED: i32 = 0;
const FONT_CHANGED: i32 = 1;
const STYLE_CHANGED: i32 = 2;

/// Applies a color scheme (again, if it's the current one: that restores its colors), then
/// the accent color, if one is set.
pub fn set_color_scheme(id: &str) -> Result<(), String> {
    // The tool refuses to apply the current scheme; forget which one is current first.
    kdelete("General", "ColorScheme");
    plasma_apply(&[id])?;
    match current_accent() {
        Some(accent) => plasma_apply(&["--accent-color", &hex(accent)]),
        None => Ok(()),
    }
}

/// Tints the Qt/KDE palette (selection, focus, links) with an accent color, or with `None`
/// goes back to the color scheme's own. Breeze icons use it for folders and highlights.
pub fn set_accent_color(accent: Option<Rgb>) -> Result<(), String> {
    match accent {
        Some(color) => {
            plasma_apply(&["--accent-color", &hex(color)])?;
            // Remembered like Plasma does, so a new scheme keeps it.
            kwrite("General", "AccentColor", &format!("{},{},{}", color.0, color.1, color.2))
        }
        None => {
            kdelete("General", "AccentColor");
            let scheme = Ini::load(&kdeglobals()).get("General", "ColorScheme").map(String::from);
            set_color_scheme(scheme.as_deref().unwrap_or("BreezeDark"))
        }
    }
}

/// The accent color set on top of the scheme, if any.
fn current_accent() -> Option<Rgb> {
    Ini::load(&kdeglobals()).get("General", "AccentColor").and_then(parse_color)
}

fn plasma_apply(args: &[&str]) -> Result<(), String> {
    let out = Command::new("plasma-apply-colorscheme")
        .args(args)
        .output()
        .map_err(|_| "Applying KDE colors needs plasma-apply-colorscheme (plasma-workspace)".to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if text.contains("already") {
        notify_kde(PALETTE_CHANGED);
        return Ok(());
    }
    Err(text.trim().to_string())
}

fn kdelete(group: &str, key: &str) {
    quiet(Command::new("kwriteconfig6").args(["--file", "kdeglobals", "--group", group, "--key", key, "--delete"]));
}

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// The SVG of the current icon theme's folder icon, if it takes the accent color (Breeze).
pub fn accent_folder_svg(icon_theme: &str) -> Option<String> {
    let dir = icon_theme_dir(icon_theme)?;
    let svg = ["places/64/folder.svg", "places/48/folder.svg", "places/32/folder.svg"]
        .iter()
        .find_map(|p| std::fs::read_to_string(dir.join(p)).ok())?;
    svg.contains("ColorScheme-Accent").then_some(svg)
}

/// The folder SVG with its accent color replaced.
pub fn tint_svg(svg: &str, color: Rgb) -> String {
    let Some(start) = svg.find(".ColorScheme-Accent") else { return svg.to_string() };
    let Some(open) = svg[start..].find('{').map(|i| start + i) else { return svg.to_string() };
    let Some(close) = svg[open..].find('}').map(|i| open + i) else { return svg.to_string() };
    format!("{}{{ color: {}; }}{}", &svg[..open], hex(color), &svg[close + 1..])
}

pub fn set_widget_style(id: &str) -> Result<(), String> {
    kwrite("KDE", "widgetStyle", id)?;
    qt6ct("style", id);
    notify_kde(STYLE_CHANGED);
    Ok(())
}

pub fn set_kvantum_theme(theme: &str) -> Result<(), String> {
    let path = kvantum_conf();
    let mut ini = Ini::load(&path);
    ini.set("General", "theme", theme);
    ini.save(&path)
}

pub fn set_icon_theme(id: &str) -> Result<(), String> {
    kwrite("Icons", "Theme", id)?;
    qt6ct("icon_theme", id);
    set_gsetting("icon-theme", id);
    gtk_ini("gtk-icon-theme-name", id);
    quiet(Command::new("dbus-send").args([
        "--session",
        "--type=signal",
        "/KIconLoader",
        "org.kde.KIconLoader.iconChanged",
        "int32:0",
    ]));
    Ok(())
}

pub fn set_gtk_theme(id: &str) -> Result<(), String> {
    set_gsetting("gtk-theme", id);
    let path = gtk_settings("gtk-3.0");
    let mut ini = Ini::load(&path);
    ini.set("Settings", "gtk-theme-name", id);
    ini.save(&path)
}

/// `default`, `prefer-dark` or `prefer-light`.
pub fn set_gtk_color_scheme(scheme: &str) -> Result<(), String> {
    set_gsetting("color-scheme", scheme);
    gtk_ini("gtk-application-prefer-dark-theme", if scheme == "prefer-dark" { "true" } else { "false" });
    Ok(())
}

pub fn set_accent(accent: &str) -> Result<(), String> {
    set_gsetting("accent-color", accent);
    Ok(())
}

/// The interface font (`monospace: false`) or the fixed-width one.
pub fn set_font(monospace: bool, family: &str, size: f64) -> Result<(), String> {
    let kde = Ini::load(&kdeglobals());
    let keys: &[&str] = if monospace { &["fixed"] } else { &["font", "menuFont", "toolBarFont"] };
    for key in keys {
        kwrite("General", key, &qt_font(kde.get("General", key), family, size))?;
    }
    let gtk = format!("{family} {}", format_size(size));
    if monospace {
        set_gsetting("monospace-font-name", &gtk);
    } else {
        set_gsetting("font-name", &gtk);
        gtk_ini("gtk-font-name", &format!("{family}, {}", format_size(size)));
    }
    notify_kde(FONT_CHANGED);
    Ok(())
}

fn format_size(size: f64) -> String {
    if size.fract() == 0.0 { format!("{size:.0}") } else { format!("{size:.1}") }
}

/// A Qt font string (`Family,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1`) with a new family and size,
/// keeping its other fields (weight, style…).
fn qt_font(old: Option<&str>, family: &str, size: f64) -> String {
    let mut fields: Vec<String> = old
        .filter(|o| o.split(',').count() >= 10)
        .unwrap_or("x,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1")
        .split(',')
        .map(String::from)
        .collect();
    fields[0] = family.to_string();
    fields[1] = format_size(size);
    fields.join(",")
}

fn parse_qt_font(s: &str) -> Option<(String, f64)> {
    let mut fields = s.split(',');
    let family = fields.next()?.trim().to_string();
    let size = fields.next()?.trim().parse().ok()?;
    (!family.is_empty()).then_some((family, size))
}

/// `"Roboto Flex 11"` (as gsettings stores fonts).
fn parse_gtk_font(s: &str) -> Option<(String, f64)> {
    let (family, size) = s.trim().rsplit_once(' ')?;
    Some((family.trim().trim_end_matches(',').to_string(), size.parse().ok()?))
}

/// Switches the `gtk.css` overrides (from theming tools) off or back on, by renaming them.
pub fn set_gtk_css(on: bool) -> Result<(), String> {
    for version in ["gtk-3.0", "gtk-4.0"] {
        let dir = crate::config_dir().join(version);
        let (from, to) = if on {
            (dir.join(GTK_CSS_OFF), dir.join("gtk.css"))
        } else {
            (dir.join("gtk.css"), dir.join(GTK_CSS_OFF))
        };
        if from.exists() && !to.exists() {
            std::fs::rename(&from, &to).map_err(|e| format!("Can't rename {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

pub fn icon_theme_dir(id: &str) -> Option<PathBuf> {
    let home = crate::home().unwrap_or_default();
    entries("icons", &[home.join(".icons")]).into_iter().find(|(n, _)| n == id).map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints_an_svg() {
        let svg = r#"<style>.ColorScheme-Accent { color: #3daee9; } .ColorScheme-Text { color: #fcfcfc; }</style>"#;
        assert_eq!(
            tint_svg(svg, (233, 84, 32)),
            r#"<style>.ColorScheme-Accent { color: #e95420; } .ColorScheme-Text { color: #fcfcfc; }</style>"#
        );
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("20,22,24"), Some((20, 22, 24)));
        assert_eq!(parse_color("#151515"), Some((0x15, 0x15, 0x15)));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn fonts() {
        assert_eq!(parse_qt_font("Roboto Flex,11,-1,5,50,0,0,0,0,0"), Some(("Roboto Flex".into(), 11.0)));
        assert_eq!(parse_gtk_font("Roboto Flex  11"), Some(("Roboto Flex".into(), 11.0)));
        assert_eq!(qt_font(Some("Roboto Flex,11,-1,5,50,0,0,0,0,0"), "Inter", 10.5), "Inter,10.5,-1,5,50,0,0,0,0,0");
        assert_eq!(qt_font(None, "Inter", 11.0), "Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    }

    #[test]
    fn scheme_preview_colors() {
        let ini = Ini::parse("[Colors:Window]\nBackgroundNormal=1,2,3\n[Colors:Selection]\nBackgroundNormal=#ff0000\n");
        let c = scheme_colors(&ini);
        assert_eq!(c.window, (1, 2, 3));
        assert_eq!(c.selection, (255, 0, 0));
        assert_eq!(c.view, (0, 0, 0));
    }
}
