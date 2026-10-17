//! dmsh's config (`~/.config/dmsh/config.kdl`): a schema of its settings, reading and
//! comment-preserving writing, validation with `dmsh config check`, previews with
//! `dmsh render`, and restarting dmsh.
//!
//! dmsh reloads the file when it's saved, so a save applies everything except `sources`,
//! which need a restart.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::kdl_text::{KdlText, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// `"#rrggbb"` or `"#rrggbbaa"`.
    Color,
    /// A font family name; unset means Inter (or the system sans-serif).
    Font,
    Text,
    Number {
        min: f64,
        max: f64,
        step: f64,
        digits: u32,
    },
    /// Written with a unit (`"1500ms"`, `"4s"`); edited in seconds.
    Duration {
        max: f64,
    },
    /// `name true|false`, or a bare `name` meaning true.
    Switch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Default {
    Str(&'static str),
    Num(f64),
    Bool(bool),
    /// No default value (the font: dmsh picks one).
    Unset,
}

#[derive(Debug)]
pub struct Setting {
    pub section: &'static str,
    pub key: &'static str,
    /// A property of the node (`accent-colors low=…`) instead of its argument.
    pub prop: Option<&'static str>,
    pub kind: Kind,
    pub default: Default,
    pub title: &'static str,
    pub subtitle: &'static str,
}

impl Setting {
    pub fn path(&self) -> [&'static str; 2] {
        [self.section, self.key]
    }

    pub fn default_value(&self) -> Option<Value> {
        match self.default {
            Default::Str(s) => Some(Value::Str(s.into())),
            Default::Num(n) => Some(Value::Float(n)),
            Default::Bool(b) => Some(Value::Bool(b)),
            Default::Unset => None,
        }
    }
}

const fn setting(
    section: &'static str,
    key: &'static str,
    kind: Kind,
    default: Default,
    title: &'static str,
    subtitle: &'static str,
) -> Setting {
    Setting { section, key, prop: None, kind, default, title, subtitle }
}

const fn px(min: f64, max: f64) -> Kind {
    Kind::Number { min, max, step: 1.0, digits: 0 }
}

const fn accent(prop: &'static str, default: &'static str, title: &'static str, subtitle: &'static str) -> Setting {
    Setting {
        section: "theme",
        key: "accent-colors",
        prop: Some(prop),
        kind: Kind::Color,
        default: Default::Str(default),
        title,
        subtitle,
    }
}

const fn source(key: &'static str, title: &'static str, subtitle: &'static str) -> Setting {
    setting("sources", key, Kind::Switch, Default::Bool(true), title, subtitle)
}

/// Every setting dmsh has except `outputs` (see [`DmshConfig::outputs`]) and `animation off`
/// (see [`DmshConfig::animations`]), in the order the docs list them.
pub static SETTINGS: &[Setting] = &[
    setting("theme", "font", Kind::Font, Default::Unset, "Font", "Inter if installed, else the system sans-serif"),
    setting("theme", "pill-color", Kind::Color, Default::Str("#000000"), "Island Color", ""),
    setting("theme", "text-color", Kind::Color, Default::Str("#f5f5f7"), "Text", "Titles and the clock"),
    setting(
        "theme",
        "secondary-text-color",
        Kind::Color,
        Default::Str("#98989d"),
        "Secondary Text",
        "Bodies, dates and counters",
    ),
    accent("low", "#8e8e93", "Low Priority", "Icons of quiet activities"),
    accent("normal", "#f5f5f7", "Normal Priority", "Most icons"),
    accent("high", "#ff9f0a", "High Priority", "Also a low battery"),
    accent("critical", "#ff453a", "Critical", "Also a critical battery"),
    setting(
        "theme",
        "reserved-height",
        px(0.0, 120.0),
        Default::Num(36.0),
        "Reserved Height",
        "Space kept free at the top; windows tile below it",
    ),
    setting(
        "theme",
        "pill-height",
        px(16.0, 80.0),
        Default::Num(30.0),
        "Island Height",
        "Must fit in the reserved height",
    ),
    setting("theme", "corner-radius", px(0.0, 100.0), Default::Num(26.0), "Corner Radius", "Of expanded islands"),
    setting(
        "theme",
        "center-idle-width",
        px(40.0, 1000.0),
        Default::Num(120.0),
        "Clock Width",
        "The center island when idle",
    ),
    setting(
        "theme",
        "center-min-width",
        px(40.0, 1000.0),
        Default::Num(160.0),
        "Activity Width",
        "The narrowest the center gets for an activity",
    ),
    setting("theme", "center-expanded-width", px(40.0, 2000.0), Default::Num(400.0), "Expanded Center Width", ""),
    setting("theme", "right-expanded-width", px(40.0, 2000.0), Default::Num(340.0), "Expanded Right Width", ""),
    setting(
        "theme",
        "right-margin",
        px(0.0, 400.0),
        Default::Num(8.0),
        "Right Margin",
        "Gap between the right island and the screen edge",
    ),
    setting(
        "clock",
        "format",
        Kind::Text,
        Default::Str("%H:%M"),
        "Time Format",
        "strftime, e.g. %I:%M %p for a 12-hour clock",
    ),
    setting(
        "clock",
        "date-format",
        Kind::Text,
        Default::Str("%A, %-d %B"),
        "Date Format",
        "Shown when the idle center expands",
    ),
    setting(
        "animation",
        "response",
        Kind::Number { min: 0.05, max: 3.0, step: 0.01, digits: 2 },
        Default::Num(0.42),
        "Response",
        "Seconds per spring oscillation; lower is snappier",
    ),
    setting(
        "animation",
        "damping",
        Kind::Number { min: 0.1, max: 2.0, step: 0.01, digits: 2 },
        Default::Num(0.78),
        "Damping",
        "1.0 means no overshoot; lower bounces more",
    ),
    setting(
        "timing",
        "osd",
        Kind::Duration { max: 60.0 },
        Default::Str("1500ms"),
        "Overlays",
        "How long volume, brightness and layout overlays stay",
    ),
    setting(
        "timing",
        "status",
        Kind::Duration { max: 120.0 },
        Default::Str("4s"),
        "Status Messages",
        "Wi-Fi and charger messages on the right island",
    ),
    setting(
        "timing",
        "notification-center",
        Kind::Duration { max: 60.0 },
        Default::Str("2s"),
        "Notifications in the Center",
        "Before a notification moves to the right island",
    ),
    source("notifications", "Notifications", "Turn off to keep another notification daemon"),
    source("tray", "System Tray", "Tray icons and menus"),
    source("media", "Media", "What's playing, with controls"),
    source("volume", "Volume", "Overlay, button and slider"),
    source("brightness", "Brightness", "Overlay, button and slider"),
    source("power", "Battery", "Level, charger messages and warnings"),
    source("network", "Wi-Fi", "Connection messages and the Wi-Fi toggle"),
    source("keyboard-layout", "Keyboard Layout", "Overlay when the layout changes"),
    source("screencast", "Screen Sharing", "Indicator while the screen is shared"),
    source("microphone", "Microphone", "Indicator while an app records"),
    source("camera", "Camera", "Indicator while an app uses a camera"),
    setting(
        "session",
        "hold",
        Kind::Duration { max: 10.0 },
        Default::Str("800ms"),
        "Hold Time",
        "How long to hold a session option before it runs",
    ),
    setting("session", "poweroff", Kind::Text, Default::Str("systemctl poweroff"), "Shut Down Command", ""),
    setting("session", "reboot", Kind::Text, Default::Str("systemctl reboot"), "Reboot Command", ""),
    setting(
        "session",
        "logout",
        Kind::Text,
        Default::Str("niri msg action quit --skip-confirmation"),
        "Log Out Command",
        "",
    ),
];

/// Seconds in a duration like `"1500ms"`, `"4s"` or `"1m"`.
pub fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, unit) = s.split_at(split);
    let n: f64 = n.trim().parse().ok()?;
    Some(match unit {
        "ms" => n / 1000.0,
        "s" => n,
        "m" => n * 60.0,
        _ => return None,
    })
}

/// Seconds as dmsh writes durations: whole seconds as `"4s"`, otherwise milliseconds.
pub fn format_duration(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round() as i64;
    if ms % 1000 == 0 { format!("{}s", ms / 1000) } else { format!("{ms}ms") }
}

pub struct DmshConfig {
    path: PathBuf,
    file: KdlText,
}

impl DmshConfig {
    /// Where dmsh reads its config: `$XDG_CONFIG_HOME/dmsh/config.kdl`.
    pub fn default_path() -> PathBuf {
        crate::config_dir().join("dmsh").join("config.kdl")
    }

    /// Reads the config. A missing file starts from dmsh's documented defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => default_text(),
            Err(e) => return Err(format!("Can't read {}: {e}", path.display())),
        };
        let file = KdlText::parse(text).map_err(|e| format!("{} isn't valid KDL ({e})", path.display()))?;
        Ok(Self { path: path.to_path_buf(), file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        self.file.text()
    }

    /// The value the file sets, if it sets one.
    pub fn get(&self, s: &Setting) -> Option<Value> {
        if s.kind == Kind::Switch && self.file.node(&s.path()).is_some_and(|n| n.entries().is_empty()) {
            return Some(Value::Bool(true));
        }
        self.file.get(&s.path(), s.prop)
    }

    /// Whether the file sets a value other than the default.
    pub fn is_custom(&self, s: &Setting) -> bool {
        let Some(value) = self.get(s) else { return false };
        match (&value, s.default_value()) {
            (_, None) => true,
            (Value::Str(a), Some(Value::Str(b))) => match s.kind {
                Kind::Color => !a.eq_ignore_ascii_case(&b),
                Kind::Duration { .. } => parse_duration(a) != parse_duration(&b),
                _ => *a != b,
            },
            (a, Some(b)) => match (a.as_f64(), b.as_f64()) {
                (Some(a), Some(b)) => (a - b).abs() > 1e-9,
                _ => *a != b,
            },
        }
    }

    /// The value in effect: the file's, else the default.
    pub fn value(&self, s: &Setting) -> Option<Value> {
        self.get(s).or_else(|| s.default_value())
    }

    pub fn set(&mut self, s: &Setting, value: &Value) -> Result<(), String> {
        let value = match (s.kind, value) {
            // Whole numbers are written without a fraction, as in dmsh's docs.
            (Kind::Number { digits: 0, .. }, Value::Float(n)) => Value::Int(n.round() as i64),
            _ => value.clone(),
        };
        self.file.set(&s.path(), s.prop, &value)
    }

    /// Goes back to the default by removing the setting from the file.
    pub fn reset(&mut self, s: &Setting) -> Result<(), String> {
        match s.prop {
            // One property of a multi-property node: put the default back in its place.
            Some(_) => match s.default_value() {
                Some(v) if self.file.node(&s.path()).is_some() => self.file.set(&s.path(), s.prop, &v),
                _ => Ok(()),
            },
            None => self.file.remove(&s.path()),
        }
    }

    pub fn animations(&self) -> bool {
        match self.file.node(&["animation", "off"]) {
            None => true,
            Some(n) if n.entries().is_empty() => false,
            Some(_) => self.file.get(&["animation", "off"], None) != Some(Value::Bool(true)),
        }
    }

    pub fn set_animations(&mut self, on: bool) -> Result<(), String> {
        if on { self.file.remove(&["animation", "off"]) } else { self.file.set_flag(&["animation", "off"]) }
    }

    /// `(only, except)`: outputs that get islands, and outputs that don't. Both empty: all.
    pub fn outputs(&self) -> (Vec<String>, Vec<String>) {
        let names =
            |key| self.file.args(&["outputs", key]).into_iter().filter_map(|v| v.as_str().map(String::from)).collect();
        (names("only"), names("except"))
    }

    /// Shows islands on every output except `hidden`.
    pub fn set_hidden_outputs(&mut self, hidden: &[String]) -> Result<(), String> {
        let (only, _) = self.outputs();
        if !only.is_empty() {
            self.file.remove(&["outputs", "only"])?;
        }
        let values: Vec<Value> = hidden.iter().map(|o| Value::Str(o.clone())).collect();
        self.file.set_args(&["outputs", "except"], &values)
    }

    /// Validates with `dmsh config check`, then replaces the file atomically (dmsh reloads it).
    pub fn save(&self) -> Result<(), String> {
        let dir = self.path.parent().ok_or("config path has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        let tmp = dir.join(".config.kdl.dm-manager");
        std::fs::write(&tmp, self.file.text()).map_err(|e| format!("Can't write {}: {e}", tmp.display()))?;
        if let Some(dmsh) = binary() {
            let check = Command::new(dmsh).arg("--config").arg(&tmp).args(["config", "check"]).output();
            if let Ok(out) = check
                && !out.status.success()
            {
                let _ = std::fs::remove_file(&tmp);
                let msg = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
                // "Error: /path/.config.kdl.dm-manager: line 2: …"
                let prefix = format!("{}: ", tmp.display());
                return Err(msg.trim().trim_start_matches("Error: ").trim_start_matches(&prefix).to_string());
            }
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("Can't save {}: {e}", self.path.display()))
    }
}

/// dmsh's documented default config, or nothing if dmsh isn't installed.
fn default_text() -> String {
    binary()
        .and_then(|b| Command::new(b).args(["config", "default"]).output().ok())
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The dmsh binary: on `PATH`, or in `~/.local/bin` (where its installer puts it).
pub fn binary() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join("dmsh"));
    let local = crate::home().map(|h| h.join(".local/bin/dmsh"));
    on_path.chain(local).find(|p| p.is_file())
}

pub fn is_running() -> bool {
    Command::new("pgrep").args(["-x", "dmsh"]).output().is_ok_and(|o| o.status.success())
}

/// Restarts dmsh: through systemd if it runs as a user service, else by asking niri to
/// start a fresh one (so it isn't a child of this app).
pub fn restart() -> Result<(), String> {
    let service = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "dmsh.service"])
        .status()
        .is_ok_and(|s| s.success());
    if service {
        return run(Command::new("systemctl").args(["--user", "restart", "dmsh.service"]));
    }
    let dmsh = binary().ok_or("dmsh isn't installed")?;
    let _ = Command::new("pkill").args(["-x", "dmsh"]).status();
    // Give the old instance a moment to release the notification name and its socket.
    std::thread::sleep(std::time::Duration::from_millis(300));
    run(Command::new("niri").args(["msg", "action", "spawn", "--"]).arg(dmsh))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// What a preview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    Idle,
    Center,
    Right,
}

/// Renders the islands with the config text into a PNG (transparent background).
pub fn render_preview(config_text: &str, preview: Preview, out: &Path) -> Result<(), String> {
    let dmsh = binary().ok_or("dmsh isn't installed")?;
    let config = out.with_extension("kdl");
    std::fs::write(&config, config_text).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(dmsh);
    cmd.arg("--config").arg(&config).arg("render").arg("-o").arg(out);
    // A small screen, cropped to the part with the islands each mode shows, so they appear
    // close to their real size. The expanded right island needs a wider one to clear the clock.
    cmd.args(["--scale", "2", "--battery", "76", "--clock", "9:41"]);
    match preview {
        Preview::Idle => cmd.args(["--width", "720", "--crop", "240,480"]),
        Preview::Center => cmd.args(["--width", "720", "--hover", "center", "--crop", "140,580"]),
        Preview::Right => cmd.args(["--width", "1000", "--crop", "400,600", "--hover", "right"]).args([
            "--wifi",
            "on",
            "--volume",
            "60",
            "--brightness",
            "80",
        ]),
    };
    run(&mut cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> DmshConfig {
        DmshConfig { path: PathBuf::from("/nonexistent/config.kdl"), file: KdlText::parse(text.into()).unwrap() }
    }

    fn find(section: &str, key: &str, prop: Option<&str>) -> &'static Setting {
        SETTINGS.iter().find(|s| s.section == section && s.key == key && s.prop == prop).unwrap()
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1500ms"), Some(1.5));
        assert_eq!(parse_duration("4s"), Some(4.0));
        assert_eq!(parse_duration("1m"), Some(60.0));
        assert_eq!(parse_duration("4"), None);
        assert_eq!(format_duration(1.5), "1500ms");
        assert_eq!(format_duration(4.0), "4s");
    }

    #[test]
    fn defaults_and_set_values() {
        let mut c = config("theme {\n    pill-height 22\n}\n");
        let height = find("theme", "pill-height", None);
        assert_eq!(c.value(height), Some(Value::Int(22)));
        let radius = find("theme", "corner-radius", None);
        assert_eq!(c.get(radius), None);
        assert_eq!(c.value(radius), Some(Value::Float(26.0)));
        c.set(radius, &Value::Float(18.0)).unwrap();
        assert_eq!(c.text(), "theme {\n    pill-height 22\n    corner-radius 18\n}\n");
        c.reset(radius).unwrap();
        assert_eq!(c.text(), "theme {\n    pill-height 22\n}\n");
    }

    #[test]
    fn custom_means_different_from_the_default() {
        let c = config(
            "theme {\n    pill-color \"#000000\"\n    pill-height 30.0\n    corner-radius 18\n}\ntiming {\n    osd \"1.5s\"\n}\n",
        );
        assert!(!c.is_custom(find("theme", "pill-color", None)));
        assert!(!c.is_custom(find("theme", "pill-height", None)));
        assert!(c.is_custom(find("theme", "corner-radius", None)));
        assert!(!c.is_custom(find("timing", "osd", None)));
        assert!(!c.is_custom(find("theme", "text-color", None)));
    }

    #[test]
    fn bare_switches_are_on() {
        let c = config("sources {\n    tray\n    media false\n}\n");
        assert_eq!(c.get(find("sources", "tray", None)), Some(Value::Bool(true)));
        assert_eq!(c.get(find("sources", "media", None)), Some(Value::Bool(false)));
        assert_eq!(c.value(find("sources", "volume", None)), Some(Value::Bool(true)));
    }

    #[test]
    fn animations_flag() {
        let mut c = config("animation {\n    response 0.42\n    // off\n}\n");
        assert!(c.animations());
        c.set_animations(false).unwrap();
        assert!(!c.animations());
        c.set_animations(true).unwrap();
        assert_eq!(c.text(), "animation {\n    response 0.42\n    // off\n}\n");
    }

    #[test]
    fn outputs() {
        let mut c = config("outputs {\n    // only \"eDP-1\"\n}\n");
        assert_eq!(c.outputs(), (vec![], vec![]));
        c.set_hidden_outputs(&["HDMI-A-1".into()]).unwrap();
        assert_eq!(c.outputs(), (vec![], vec!["HDMI-A-1".to_string()]));
        c.set_hidden_outputs(&[]).unwrap();
        assert_eq!(c.outputs(), (vec![], vec![]));
    }

    #[test]
    fn resetting_an_accent_keeps_the_others() {
        let mut c = config("theme {\n    accent-colors low=\"#000000\" normal=\"#ffffff\"\n}\n");
        c.reset(find("theme", "accent-colors", Some("low"))).unwrap();
        assert_eq!(c.text(), "theme {\n    accent-colors low=\"#8e8e93\" normal=\"#ffffff\"\n}\n");
    }
}
