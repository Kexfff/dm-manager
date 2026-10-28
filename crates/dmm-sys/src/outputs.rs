//! Monitors: what niri reports (`niri msg outputs`), changing them live (`niri msg output`,
//! which lasts until the config changes), and the `output` blocks dm-manager writes.

use std::process::Command;

use crate::kdl_text::{Value, Version};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Millihertz.
    pub refresh: u32,
    pub preferred: bool,
}

impl Mode {
    /// "2880 × 1800 · 120 Hz"
    pub fn label(&self) -> String {
        format!("{} × {} · {} Hz", self.width, self.height, hz(self.refresh))
    }

    /// As niri's config and `niri msg output … mode` take it: `2880x1800@120.000`.
    pub fn spec(&self) -> String {
        format!("{}x{}@{:.3}", self.width, self.height, f64::from(self.refresh) / 1000.0)
    }
}

/// "120", "59.95"
fn hz(millihertz: u32) -> String {
    let hz = f64::from(millihertz) / 1000.0;
    if (hz - hz.round()).abs() < 0.02 { format!("{:.0}", hz.round()) } else { format!("{hz:.2}") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    Normal,
    R90,
    R180,
    R270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::R90,
        Self::R180,
        Self::R270,
        Self::Flipped,
        Self::Flipped90,
        Self::Flipped180,
        Self::Flipped270,
    ];

    /// niri's name, in the config and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::R90 => "90",
            Self::R180 => "180",
            Self::R270 => "270",
            Self::Flipped => "flipped",
            Self::Flipped90 => "flipped-90",
            Self::Flipped180 => "flipped-180",
            Self::Flipped270 => "flipped-270",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::R90 => "90°",
            Self::R180 => "180°",
            Self::R270 => "270°",
            Self::Flipped => "Flipped",
            Self::Flipped90 => "Flipped, 90°",
            Self::Flipped180 => "Flipped, 180°",
            Self::Flipped270 => "Flipped, 270°",
        }
    }

    /// The JSON names (`Normal`, `_90`, `Flipped90`) or the config's.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('_').to_lowercase().replace('-', "");
        Self::ALL.into_iter().find(|t| t.name().replace('-', "") == s)
    }

    /// Turned a quarter: width and height swap.
    pub fn sideways(self) -> bool {
        matches!(self, Self::R90 | Self::R270 | Self::Flipped90 | Self::Flipped270)
    }
}

/// How a monitor is (or should be) set up.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub on: bool,
    pub mode: Option<Mode>,
    pub scale: f64,
    pub transform: Transform,
    pub x: i32,
    pub y: i32,
    pub vrr: bool,
}

impl Settings {
    /// The size it takes in the layout (logical pixels), from its mode, scale and rotation.
    pub fn logical_size(&self) -> (f64, f64) {
        let Some(mode) = self.mode else { return (0.0, 0.0) };
        let (w, h) = (f64::from(mode.width) / self.scale, f64::from(mode.height) / self.scale);
        if self.transform.sideways() { (h, w) } else { (w, h) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// The connector, e.g. `eDP-1`.
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: Option<String>,
    pub modes: Vec<Mode>,
    pub current_mode: Option<usize>,
    pub vrr_supported: bool,
    pub vrr_enabled: bool,
    /// Millimeters.
    pub physical_size: Option<(u32, u32)>,
    /// Where it is in the layout; `None` when it's off.
    pub logical: Option<Logical>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Logical {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub transform: Transform,
}

impl Output {
    /// "Dell U2723QE", or the connector when the monitor doesn't say.
    pub fn description(&self) -> String {
        let make = self.make.split(',').next().unwrap_or(&self.make).trim();
        let text = format!("{make} {}", self.model).trim().to_string();
        if text.is_empty() || text.eq_ignore_ascii_case("unknown") { self.name.clone() } else { text }
    }

    /// The built-in screen of a laptop.
    pub fn is_internal(&self) -> bool {
        let n = self.name.to_lowercase();
        n.starts_with("edp") || n.starts_with("lvds") || n.starts_with("dsi")
    }

    /// Its settings now.
    pub fn settings(&self) -> Settings {
        let l = self.logical.as_ref();
        Settings {
            on: l.is_some(),
            mode: self.current_mode.and_then(|i| self.modes.get(i)).copied().or_else(|| self.preferred_mode()),
            scale: l.map_or(1.0, |l| l.scale),
            transform: l.map_or(Transform::Normal, |l| l.transform),
            x: l.map_or(0, |l| l.x),
            y: l.map_or(0, |l| l.y),
            vrr: self.vrr_enabled,
        }
    }

    pub fn preferred_mode(&self) -> Option<Mode> {
        self.modes.iter().find(|m| m.preferred).or(self.modes.first()).copied()
    }
}

/// The connected monitors, from niri.
pub fn read() -> Result<Vec<Output>, String> {
    let out =
        Command::new("niri").args(["msg", "--json", "outputs"]).output().map_err(|e| format!("Can't run niri: {e}"))?;
    if !out.status.success() {
        return Err("niri isn't running (this page needs a niri session)".into());
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

fn parse(json: &str) -> Result<Vec<Output>, String> {
    let json: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut outputs: Vec<Output> = json
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, o)| {
            let text = |k: &str| o.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let modes = o
                .get("modes")
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some(Mode {
                        width: m.get("width")?.as_u64()? as u32,
                        height: m.get("height")?.as_u64()? as u32,
                        refresh: m.get("refresh_rate")?.as_u64()? as u32,
                        preferred: m.get("is_preferred").and_then(|p| p.as_bool()).unwrap_or(false),
                    })
                })
                .collect();
            let logical = o.get("logical").filter(|l| !l.is_null()).and_then(|l| {
                Some(Logical {
                    x: l.get("x")?.as_i64()? as i32,
                    y: l.get("y")?.as_i64()? as i32,
                    width: l.get("width")?.as_u64()? as u32,
                    height: l.get("height")?.as_u64()? as u32,
                    scale: l.get("scale")?.as_f64()?,
                    transform: Transform::parse(l.get("transform")?.as_str()?).unwrap_or(Transform::Normal),
                })
            });
            let size = o
                .get("physical_size")
                .and_then(|s| s.as_array())
                .and_then(|s| Some((s.first()?.as_u64()? as u32, s.get(1)?.as_u64()? as u32)));
            Output {
                name: name.clone(),
                make: text("make"),
                model: text("model"),
                serial: o.get("serial").and_then(|s| s.as_str()).map(String::from),
                modes,
                current_mode: o.get("current_mode").and_then(|m| m.as_u64()).map(|m| m as usize),
                vrr_supported: o.get("vrr_supported").and_then(|v| v.as_bool()).unwrap_or(false),
                vrr_enabled: o.get("vrr_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
                physical_size: size.filter(|(w, h)| *w > 0 && *h > 0),
                logical,
            }
        })
        .collect();
    // The built-in screen first, then by name.
    outputs.sort_by(|a, b| b.is_internal().cmp(&a.is_internal()).then_with(|| a.name.cmp(&b.name)));
    Ok(outputs)
}

/// Changes a monitor live, from `from` to `to` (only what differs). niri forgets this when
/// its config next changes, so the config must be written to keep it.
pub fn apply(name: &str, from: &Settings, to: &Settings) -> Result<(), String> {
    let mut commands: Vec<Vec<String>> = Vec::new();
    if to.on && !from.on {
        commands.push(vec!["on".into()]);
    }
    if to.on {
        if let Some(mode) = to.mode
            && Some(mode) != from.mode
        {
            commands.push(vec!["mode".into(), mode.spec()]);
        }
        if (to.scale - from.scale).abs() > 1e-6 {
            commands.push(vec!["scale".into(), format!("{}", to.scale)]);
        }
        if to.transform != from.transform {
            commands.push(vec!["transform".into(), to.transform.name().into()]);
        }
        if (to.x, to.y) != (from.x, from.y) {
            commands.push(vec!["position".into(), "set".into(), to.x.to_string(), to.y.to_string()]);
        }
        if to.vrr != from.vrr {
            commands.push(vec!["vrr".into(), if to.vrr { "on" } else { "off" }.into()]);
        }
    } else if from.on {
        commands.push(vec!["off".into()]);
    }
    for args in commands {
        let out = Command::new("niri")
            .args(["msg", "output", name])
            .args(&args)
            .output()
            .map_err(|e| format!("Can't run niri: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("niri refused `{}`: {}", args.join(" "), err.lines().last().unwrap_or("").trim()));
        }
    }
    Ok(())
}

/// The settings dm-manager writes in an output block; other children are kept as they are.
pub const MANAGED: [&str; 6] = ["mode", "scale", "transform", "position", "variable-refresh-rate", "off"];

/// An `output "name" { … }` block for these settings, with `extra` (other children, as
/// written) kept at the end.
pub fn render_block(name: &str, s: &Settings, extra: &[String]) -> String {
    let mut lines = vec![format!("output {} {{", Value::Str(name.into()).repr(Version::V1))];
    if !s.on {
        lines.push("    off".into());
    } else {
        if let Some(mode) = s.mode {
            lines.push(format!("    mode \"{}\"", mode.spec()));
        }
        lines.push(format!("    scale {}", Value::Float(s.scale).repr(Version::V1)));
        lines.push(format!("    transform \"{}\"", s.transform.name()));
        lines.push(format!("    position x={} y={}", s.x, s.y));
        if s.vrr {
            lines.push("    variable-refresh-rate".into());
        }
    }
    lines.extend(extra.iter().map(|l| format!("    {}", l.trim())));
    lines.push("}".into());
    lines.join("\n")
}

/// Common scales, plus whatever is set now.
pub fn scales(current: f64) -> Vec<f64> {
    let mut list = vec![1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
    if !list.iter().any(|s| (s - current).abs() < 1e-6) {
        list.push(current);
        list.sort_by(f64::total_cmp);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"eDP-1":{"name":"eDP-1","make":"China Star Optoelectronics Technology Co., Ltd","model":"MNE007ZA3-5",
        "serial":null,"physical_size":[300,190],"modes":[{"width":2880,"height":1800,"refresh_rate":120000,"is_preferred":true},
        {"width":2880,"height":1800,"refresh_rate":60001,"is_preferred":true}],"current_mode":0,"is_custom_mode":false,
        "vrr_supported":true,"vrr_enabled":true,"logical":{"x":0,"y":0,"width":1645,"height":1028,"scale":1.75,"transform":"Normal"}},
        "HDMI-A-1":{"name":"HDMI-A-1","make":"Dell Inc.","model":"U2723QE","serial":"X","physical_size":[600,340],
        "modes":[{"width":3840,"height":2160,"refresh_rate":59997,"is_preferred":true}],"current_mode":null,
        "vrr_supported":false,"vrr_enabled":false,"logical":null}}"#;

    #[test]
    fn parses_niri_outputs() {
        let outputs = parse(JSON).unwrap();
        assert_eq!(outputs[0].name, "eDP-1");
        assert_eq!(outputs[0].description(), "China Star Optoelectronics Technology Co. MNE007ZA3-5");
        let s = outputs[0].settings();
        assert_eq!((s.on, s.scale, s.transform, s.vrr), (true, 1.75, Transform::Normal, true));
        assert_eq!(s.mode.unwrap().label(), "2880 × 1800 · 120 Hz");
        let off = outputs[1].settings();
        assert!(!off.on);
        assert_eq!(off.mode.unwrap().spec(), "3840x2160@59.997");
    }

    #[test]
    fn transforms() {
        assert_eq!(Transform::parse("_90"), Some(Transform::R90));
        assert_eq!(Transform::parse("Flipped270"), Some(Transform::Flipped270));
        assert_eq!(Transform::parse("flipped-90"), Some(Transform::Flipped90));
        assert_eq!(Transform::parse("Normal"), Some(Transform::Normal));
    }

    #[test]
    fn logical_size() {
        let mut s = parse(JSON).unwrap()[0].settings();
        assert_eq!(s.logical_size(), (2880.0 / 1.75, 1800.0 / 1.75));
        s.transform = Transform::R90;
        assert_eq!(s.logical_size(), (1800.0 / 1.75, 2880.0 / 1.75));
    }

    #[test]
    fn renders_a_block() {
        let s = parse(JSON).unwrap()[0].settings();
        let block = render_block("eDP-1", &s, &["focus-at-startup".into()]);
        assert_eq!(
            block,
            "output \"eDP-1\" {\n    mode \"2880x1800@120.000\"\n    scale 1.75\n    transform \"normal\"\n    position x=0 y=0\n    variable-refresh-rate\n    focus-at-startup\n}"
        );
        assert!(kdl::KdlDocument::parse_v1(&block).is_ok());
        let off = Settings { on: false, ..s };
        assert_eq!(render_block("eDP-1", &off, &[]), "output \"eDP-1\" {\n    off\n}");
    }
}
