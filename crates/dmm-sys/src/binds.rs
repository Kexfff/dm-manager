//! niri key binds: the actions a bind can run (a catalog with friendly names, generated from
//! `niri msg action --help`), turning actions into niri's syntax and back, and writing binds.

use kdl::KdlDocument;

use crate::kdl_text::{Value, Version};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Apps,
    Windows,
    Columns,
    Workspaces,
    Monitors,
    Screenshots,
    System,
}

impl Category {
    pub const ALL: [Self; 7] =
        [Self::Apps, Self::Windows, Self::Columns, Self::Workspaces, Self::Monitors, Self::Screenshots, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Apps => "Apps & Commands",
            Self::Windows => "Windows",
            Self::Columns => "Columns",
            Self::Workspaces => "Workspaces",
            Self::Monitors => "Monitors",
            Self::Screenshots => "Screenshots & Screencasts",
            Self::System => "System",
        }
    }
}

/// What an action takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    None,
    /// A program and its arguments (`spawn "kitty" "-e" "htop"`).
    Command,
    /// A shell command line (`spawn-sh "grim - | wl-copy"`).
    Shell,
    /// A whole number.
    Number,
    /// A workspace index or name.
    Workspace,
    /// A string, described by the hint.
    Text(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// niri's name for it.
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub arg: Arg,
}

const fn action(id: &'static str, label: &'static str, category: Category, arg: Arg) -> Action {
    Action { id, label, category, arg }
}

/// Every action niri can bind (except debugging ones), in niri's order.
pub static ACTIONS: &[Action] = &[
    action("quit", "Exit niri", Category::System, Arg::None),
    action("power-off-monitors", "Power off all monitors via DPMS", Category::System, Arg::None),
    action("power-on-monitors", "Power on all monitors via DPMS", Category::System, Arg::None),
    action("spawn", "Run a program", Category::Apps, Arg::Command),
    action("spawn-sh", "Run a shell command", Category::Apps, Arg::Shell),
    action("do-screen-transition", "Do a screen transition", Category::System, Arg::None),
    action("screenshot", "Open the screenshot UI", Category::Screenshots, Arg::None),
    action("screenshot-screen", "Screenshot screen", Category::Screenshots, Arg::None),
    action("screenshot-window", "Screenshot window", Category::Screenshots, Arg::None),
    action(
        "toggle-keyboard-shortcuts-inhibit",
        "Enable or disable the keyboard shortcuts inhibitor for surface",
        Category::System,
        Arg::None,
    ),
    action("close-window", "Close window", Category::Windows, Arg::None),
    action("fullscreen-window", "Toggle fullscreen on window", Category::Windows, Arg::None),
    action("toggle-windowed-fullscreen", "Toggle windowed fullscreen on window", Category::Windows, Arg::None),
    action("focus-window", "Focus window by id", Category::Windows, Arg::Number),
    action("focus-window-in-column", "Focus window in column by index", Category::Columns, Arg::Number),
    action("focus-window-previous", "Focus the previously focused window", Category::Windows, Arg::None),
    action("focus-column-left", "Focus the column to the left", Category::Columns, Arg::None),
    action("focus-column-right", "Focus the column to the right", Category::Columns, Arg::None),
    action("focus-column-first", "Focus the first column", Category::Columns, Arg::None),
    action("focus-column-last", "Focus the last column", Category::Columns, Arg::None),
    action(
        "focus-column-right-or-first",
        "Focus the next column to the right, looping if at end",
        Category::Columns,
        Arg::None,
    ),
    action(
        "focus-column-left-or-last",
        "Focus the next column to the left, looping if at start",
        Category::Columns,
        Arg::None,
    ),
    action("focus-column", "Focus column by index", Category::Columns, Arg::Number),
    action("focus-window-or-monitor-up", "Focus the window or the monitor above", Category::Monitors, Arg::None),
    action("focus-window-or-monitor-down", "Focus the window or the monitor below", Category::Monitors, Arg::None),
    action(
        "focus-column-or-monitor-left",
        "Focus the column or the monitor to the left",
        Category::Monitors,
        Arg::None,
    ),
    action(
        "focus-column-or-monitor-right",
        "Focus the column or the monitor to the right",
        Category::Monitors,
        Arg::None,
    ),
    action("focus-window-down", "Focus the window below", Category::Windows, Arg::None),
    action("focus-window-up", "Focus the window above", Category::Windows, Arg::None),
    action(
        "focus-window-down-or-column-left",
        "Focus the window below or the column to the left",
        Category::Columns,
        Arg::None,
    ),
    action(
        "focus-window-down-or-column-right",
        "Focus the window below or the column to the right",
        Category::Columns,
        Arg::None,
    ),
    action(
        "focus-window-up-or-column-left",
        "Focus the window above or the column to the left",
        Category::Columns,
        Arg::None,
    ),
    action(
        "focus-window-up-or-column-right",
        "Focus the window above or the column to the right",
        Category::Columns,
        Arg::None,
    ),
    action(
        "focus-window-or-workspace-down",
        "Focus the window or the workspace below",
        Category::Workspaces,
        Arg::None,
    ),
    action("focus-window-or-workspace-up", "Focus the window or the workspace above", Category::Workspaces, Arg::None),
    action("focus-window-top", "Focus the topmost window", Category::Windows, Arg::None),
    action("focus-window-bottom", "Focus the bottommost window", Category::Windows, Arg::None),
    action("focus-window-down-or-top", "Focus the window below or the topmost window", Category::Windows, Arg::None),
    action(
        "focus-window-up-or-bottom",
        "Focus the window above or the bottommost window",
        Category::Windows,
        Arg::None,
    ),
    action("move-column-left", "Move column to the left", Category::Columns, Arg::None),
    action("move-column-right", "Move column to the right", Category::Columns, Arg::None),
    action("move-column-to-first", "Move column to the start of the workspace", Category::Columns, Arg::None),
    action("move-column-to-last", "Move column to the end of the workspace", Category::Columns, Arg::None),
    action(
        "move-column-left-or-to-monitor-left",
        "Move column to the left or to the monitor to the left",
        Category::Monitors,
        Arg::None,
    ),
    action(
        "move-column-right-or-to-monitor-right",
        "Move column to the right or to the monitor to the right",
        Category::Monitors,
        Arg::None,
    ),
    action("move-column-to-index", "Move column to a specific index on its workspace", Category::Columns, Arg::Number),
    action("move-window-down", "Move window down in column", Category::Windows, Arg::None),
    action("move-window-up", "Move window up in column", Category::Windows, Arg::None),
    action(
        "move-window-down-or-to-workspace-down",
        "Move window down in column or to the workspace below",
        Category::Workspaces,
        Arg::None,
    ),
    action(
        "move-window-up-or-to-workspace-up",
        "Move window up in column or to the workspace above",
        Category::Workspaces,
        Arg::None,
    ),
    action("consume-or-expel-window-left", "Consume or expel window left", Category::Windows, Arg::None),
    action("consume-or-expel-window-right", "Consume or expel window right", Category::Windows, Arg::None),
    action("consume-window-into-column", "Consume the window to the right into column", Category::Columns, Arg::None),
    action("expel-window-from-column", "Expel the bottom window from column", Category::Columns, Arg::None),
    action("swap-window-right", "Swap focused window with one to the right", Category::Windows, Arg::None),
    action("swap-window-left", "Swap focused window with one to the left", Category::Windows, Arg::None),
    action(
        "toggle-column-tabbed-display",
        "Toggle column between normal and tabbed display",
        Category::Columns,
        Arg::None,
    ),
    action("set-column-display", "Set the display mode of column", Category::Columns, Arg::Text("normal or tabbed")),
    action("center-column", "Center column on the screen", Category::Columns, Arg::None),
    action("center-window", "Center window on the screen", Category::Windows, Arg::None),
    action("center-visible-columns", "Center all fully visible columns on the screen", Category::Columns, Arg::None),
    action("focus-workspace-down", "Focus the workspace below", Category::Workspaces, Arg::None),
    action("focus-workspace-up", "Focus the workspace above", Category::Workspaces, Arg::None),
    action("focus-workspace", "Focus workspace", Category::Workspaces, Arg::Workspace),
    action("focus-workspace-previous", "Focus the previous workspace", Category::Workspaces, Arg::None),
    action("move-window-to-workspace-down", "Move window to the workspace below", Category::Workspaces, Arg::None),
    action("move-window-to-workspace-up", "Move window to the workspace above", Category::Workspaces, Arg::None),
    action("move-window-to-workspace", "Move window to workspace", Category::Workspaces, Arg::Workspace),
    action("move-column-to-workspace-down", "Move column to the workspace below", Category::Workspaces, Arg::None),
    action("move-column-to-workspace-up", "Move column to the workspace above", Category::Workspaces, Arg::None),
    action("move-column-to-workspace", "Move column to workspace", Category::Workspaces, Arg::Workspace),
    action("move-workspace-down", "Move workspace down", Category::Workspaces, Arg::None),
    action("move-workspace-up", "Move workspace up", Category::Workspaces, Arg::None),
    action(
        "move-workspace-to-index",
        "Move workspace to a specific index on its monitor",
        Category::Workspaces,
        Arg::Number,
    ),
    action("set-workspace-name", "Set the name of workspace", Category::Workspaces, Arg::Text("Name")),
    action("unset-workspace-name", "Unset the name of workspace", Category::Workspaces, Arg::None),
    action("focus-monitor-left", "Focus the monitor to the left", Category::Monitors, Arg::None),
    action("focus-monitor-right", "Focus the monitor to the right", Category::Monitors, Arg::None),
    action("focus-monitor-down", "Focus the monitor below", Category::Monitors, Arg::None),
    action("focus-monitor-up", "Focus the monitor above", Category::Monitors, Arg::None),
    action("focus-monitor-previous", "Focus the previous monitor", Category::Monitors, Arg::None),
    action("focus-monitor-next", "Focus the next monitor", Category::Monitors, Arg::None),
    action("focus-monitor", "Focus monitor by name", Category::Monitors, Arg::Text("Monitor, e.g. HDMI-A-1")),
    action("move-window-to-monitor-left", "Move window to the monitor to the left", Category::Monitors, Arg::None),
    action("move-window-to-monitor-right", "Move window to the monitor to the right", Category::Monitors, Arg::None),
    action("move-window-to-monitor-down", "Move window to the monitor below", Category::Monitors, Arg::None),
    action("move-window-to-monitor-up", "Move window to the monitor above", Category::Monitors, Arg::None),
    action("move-window-to-monitor-previous", "Move window to the previous monitor", Category::Monitors, Arg::None),
    action("move-window-to-monitor-next", "Move window to the next monitor", Category::Monitors, Arg::None),
    action("move-window-to-monitor", "Move window to monitor", Category::Monitors, Arg::Text("Monitor, e.g. HDMI-A-1")),
    action("move-column-to-monitor-left", "Move column to the monitor to the left", Category::Monitors, Arg::None),
    action("move-column-to-monitor-right", "Move column to the monitor to the right", Category::Monitors, Arg::None),
    action("move-column-to-monitor-down", "Move column to the monitor below", Category::Monitors, Arg::None),
    action("move-column-to-monitor-up", "Move column to the monitor above", Category::Monitors, Arg::None),
    action("move-column-to-monitor-previous", "Move column to the previous monitor", Category::Monitors, Arg::None),
    action("move-column-to-monitor-next", "Move column to the next monitor", Category::Monitors, Arg::None),
    action("move-column-to-monitor", "Move column to monitor", Category::Monitors, Arg::Text("Monitor, e.g. HDMI-A-1")),
    action("set-window-width", "Set window width", Category::Windows, Arg::Text("Width, e.g. +10%, -100 or 50%")),
    action("set-window-height", "Set window height", Category::Windows, Arg::Text("Height, e.g. +10%, -100 or 50%")),
    action("reset-window-height", "Reset the height of window back to automatic", Category::Windows, Arg::None),
    action("switch-preset-column-width", "Switch between preset column widths", Category::Columns, Arg::None),
    action(
        "switch-preset-column-width-back",
        "Switch between preset column widths backwards",
        Category::Columns,
        Arg::None,
    ),
    action("switch-preset-window-width", "Switch between preset window widths", Category::Windows, Arg::None),
    action(
        "switch-preset-window-width-back",
        "Switch between preset window widths backwards",
        Category::Windows,
        Arg::None,
    ),
    action("switch-preset-window-height", "Switch between preset window heights", Category::Windows, Arg::None),
    action(
        "switch-preset-window-height-back",
        "Switch between preset window heights backwards",
        Category::Windows,
        Arg::None,
    ),
    action("maximize-column", "Toggle the maximized state of column", Category::Columns, Arg::None),
    action("maximize-window-to-edges", "Toggle the maximized-to-edges state of window", Category::Windows, Arg::None),
    action("set-column-width", "Set column width", Category::Columns, Arg::Text("Width, e.g. +10%, -100 or 50%")),
    action(
        "expand-column-to-available-width",
        "Expand column to space not taken up by other fully visible columns",
        Category::Columns,
        Arg::None,
    ),
    action(
        "switch-layout",
        "Switch between keyboard layouts",
        Category::System,
        Arg::Text("next, prev, or a layout number"),
    ),
    action("show-hotkey-overlay", "Show the hotkey overlay", Category::System, Arg::None),
    action(
        "move-workspace-to-monitor-left",
        "Move workspace to the monitor to the left",
        Category::Monitors,
        Arg::None,
    ),
    action(
        "move-workspace-to-monitor-right",
        "Move workspace to the monitor to the right",
        Category::Monitors,
        Arg::None,
    ),
    action("move-workspace-to-monitor-down", "Move workspace to the monitor below", Category::Monitors, Arg::None),
    action("move-workspace-to-monitor-up", "Move workspace to the monitor above", Category::Monitors, Arg::None),
    action(
        "move-workspace-to-monitor-previous",
        "Move workspace to the previous monitor",
        Category::Monitors,
        Arg::None,
    ),
    action("move-workspace-to-monitor-next", "Move workspace to the next monitor", Category::Monitors, Arg::None),
    action(
        "move-workspace-to-monitor",
        "Move workspace to monitor",
        Category::Monitors,
        Arg::Text("Monitor, e.g. HDMI-A-1"),
    ),
    action(
        "toggle-window-floating",
        "Move window between the floating and the tiling layout",
        Category::Windows,
        Arg::None,
    ),
    action("move-window-to-floating", "Move window to the floating layout", Category::Windows, Arg::None),
    action("move-window-to-tiling", "Move window to the tiling layout", Category::Windows, Arg::None),
    action("focus-floating", "Switches focus to the floating layout", Category::Windows, Arg::None),
    action("focus-tiling", "Switches focus to the tiling layout", Category::Windows, Arg::None),
    action(
        "switch-focus-between-floating-and-tiling",
        "Toggles the focus between the floating and the tiling layout",
        Category::Windows,
        Arg::None,
    ),
    action("move-floating-window", "Move the floating window on screen", Category::Windows, Arg::None),
    action("toggle-window-rule-opacity", "Toggle the opacity of window", Category::Windows, Arg::None),
    action("set-dynamic-cast-window", "Set the dynamic cast target to window", Category::Screenshots, Arg::None),
    action("set-dynamic-cast-monitor", "Set the dynamic cast target to monitor", Category::Screenshots, Arg::None),
    action(
        "clear-dynamic-cast-target",
        "Clear the dynamic cast target, making it show nothing",
        Category::Screenshots,
        Arg::None,
    ),
    action("stop-cast", "Stop a PipeWire screencast", Category::Screenshots, Arg::None),
    action("toggle-overview", "Toggle the Overview", Category::System, Arg::None),
    action("open-overview", "Open the Overview", Category::System, Arg::None),
    action("close-overview", "Close the Overview", Category::System, Arg::None),
    action("toggle-window-urgent", "Toggle urgent status of window", Category::Windows, Arg::None),
    action("set-window-urgent", "Set urgent status of window", Category::Windows, Arg::None),
    action("unset-window-urgent", "Unset urgent status of window", Category::Windows, Arg::None),
    action("load-config-file", "Reload the config file", Category::System, Arg::None),
];

pub fn find(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// An action as written, split into its catalog entry (if known) and argument.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub id: String,
    pub action: Option<&'static Action>,
    /// The argument as a user edits it: a command line, a number, a name.
    pub arg: String,
    /// It has properties or arguments the catalog can't show (edit it as raw text).
    pub custom: bool,
}

/// Splits `spawn "kitty" "-e" "htop"` into the action and `kitty -e htop`.
pub fn parse(text: &str) -> Option<Parsed> {
    let doc = KdlDocument::parse_v1(text.trim()).or_else(|_| KdlDocument::parse_v2(text.trim())).ok()?;
    let node = doc.nodes().first()?;
    let id = node.name().value().to_string();
    let action = find(&id);
    let mut args = Vec::new();
    let mut custom = node.children().is_some();
    for entry in node.entries() {
        if entry.name().is_some() {
            custom = true;
            continue;
        }
        args.push(match entry.value() {
            kdl::KdlValue::String(s) if s.contains(char::is_whitespace) && id == "spawn" => format!("\"{s}\""),
            kdl::KdlValue::String(s) => s.clone(),
            other => other.to_string(),
        });
    }
    let fits = match action.map(|a| a.arg) {
        Some(Arg::None) => args.is_empty(),
        Some(Arg::Command) => true,
        Some(_) => args.len() <= 1,
        None => false,
    };
    Some(Parsed { id, action, arg: args.join(" "), custom: custom || !fits })
}

/// The action in niri's syntax, from its catalog entry and an argument as typed.
pub fn build(action: &Action, arg: &str) -> Result<String, String> {
    let arg = arg.trim();
    let quoted = |s: &str| Value::Str(s.to_string()).repr(Version::V1);
    let needs = |what: &str| if arg.is_empty() { Err(format!("Enter {what}")) } else { Ok(()) };
    Ok(match action.arg {
        Arg::None => action.id.to_string(),
        Arg::Command => {
            needs("a program to run")?;
            let words = split_command(arg).ok_or("Unbalanced quotes in the command")?;
            let words: Vec<String> = words.iter().map(|w| quoted(w)).collect();
            format!("{} {}", action.id, words.join(" "))
        }
        Arg::Shell => {
            needs("a command")?;
            format!("{} {}", action.id, quoted(arg))
        }
        Arg::Number => {
            let n: u64 = arg.parse().map_err(|_| "Enter a whole number".to_string())?;
            format!("{} {n}", action.id)
        }
        Arg::Workspace => {
            needs("a workspace number or name")?;
            match arg.parse::<u64>() {
                Ok(n) => format!("{} {n}", action.id),
                Err(_) => format!("{} {}", action.id, quoted(arg)),
            }
        }
        Arg::Text(_) => {
            needs("a value")?;
            format!("{} {}", action.id, quoted(arg))
        }
    })
}

/// Splits a command line into words, honoring "double" and 'single' quotes.
pub fn split_command(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            (None, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// A short description of a bind's action, for lists: "Run kitty", "Focus workspace 3".
pub fn describe(action_text: &str) -> String {
    let Some(p) = parse(action_text) else { return action_text.to_string() };
    let Some(action) = p.action else { return action_text.to_string() };
    if p.arg.is_empty() || p.custom {
        return if p.custom { action_text.to_string() } else { action.label.to_string() };
    }
    match action.arg {
        Arg::Command => {
            // The program by its name, with its arguments: "Run playerctl play-pause".
            let words = split_command(&p.arg).unwrap_or_default();
            let mut words = words.into_iter();
            let program = words.next().unwrap_or_default();
            let program = program.rsplit('/').next().unwrap_or(&program).to_string();
            let line = std::iter::once(program).chain(words).collect::<Vec<_>>().join(" ");
            format!("Run {}", shorten(&line, 48))
        }
        Arg::Shell => format!("Run: {}", shorten(&p.arg, 48)),
        Arg::Workspace => format!("{} {}", action.label, p.arg),
        _ => format!("{}: {}", action.label, p.arg),
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max - 1).collect::<String>() + "…"
}

/// A bind as dm-manager writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct BindSpec {
    pub key: String,
    /// In niri's syntax, e.g. `spawn "kitty"`.
    pub action: String,
    pub title: Option<String>,
    pub repeat: bool,
    pub allow_when_locked: bool,
}

impl BindSpec {
    /// `Mod+T hotkey-overlay-title="Terminal" { spawn "kitty"; }`
    pub fn render(&self) -> String {
        let mut line = key_name(&self.key);
        if let Some(title) = self.title.as_deref().filter(|t| !t.trim().is_empty()) {
            line.push_str(&format!(" hotkey-overlay-title={}", Value::Str(title.trim().to_string()).repr(Version::V1)));
        }
        if !self.repeat {
            line.push_str(" repeat=false");
        }
        if self.allow_when_locked {
            line.push_str(" allow-when-locked=true");
        }
        format!("{line} {{ {}; }}", self.action.trim().trim_end_matches(';'))
    }
}

/// A key as a node name: bare when KDL allows it (it nearly always does), else quoted.
fn key_name(key: &str) -> String {
    let bare = !key.is_empty()
        && !key.chars().any(|c| c.is_whitespace() || "(){}[]<>;=,\"\\/#".contains(c))
        && !key.starts_with(|c: char| c.is_ascii_digit());
    if bare { key.to_string() } else { Value::Str(key.to_string()).repr(Version::V1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog() {
        assert!(ACTIONS.len() > 100);
        assert_eq!(find("close-window").unwrap().label, "Close window");
        assert_eq!(find("spawn").unwrap().arg, Arg::Command);
    }

    #[test]
    fn parses_actions() {
        let p = parse(r#"spawn "kitty" "-e" "htop""#).unwrap();
        assert_eq!((p.id.as_str(), p.arg.as_str(), p.custom), ("spawn", "kitty -e htop", false));
        let p = parse("focus-workspace 3").unwrap();
        assert_eq!((p.arg.as_str(), p.custom), ("3", false));
        assert!(parse("screenshot show-pointer=false").unwrap().custom);
        assert!(parse("some-future-action").unwrap().action.is_none());
    }

    #[test]
    fn builds_actions() {
        assert_eq!(
            build(find("spawn").unwrap(), r#"kitty -e "my script""#).unwrap(),
            r#"spawn "kitty" "-e" "my script""#
        );
        assert_eq!(build(find("spawn-sh").unwrap(), "grim - | wl-copy").unwrap(), r#"spawn-sh "grim - | wl-copy""#);
        assert_eq!(build(find("focus-workspace").unwrap(), "3").unwrap(), "focus-workspace 3");
        assert_eq!(build(find("focus-workspace").unwrap(), "chat").unwrap(), r#"focus-workspace "chat""#);
        assert_eq!(build(find("close-window").unwrap(), "").unwrap(), "close-window");
        assert!(build(find("focus-column").unwrap(), "x").is_err());
        assert!(build(find("spawn").unwrap(), "").is_err());
    }

    #[test]
    fn round_trip() {
        for text in [r#"spawn "kitty""#, "focus-workspace 3", r#"set-column-width "+10%""#, "close-window"] {
            let p = parse(text).unwrap();
            assert_eq!(build(p.action.unwrap(), &p.arg).unwrap(), text);
        }
    }

    #[test]
    fn descriptions() {
        assert_eq!(describe(r#"spawn "/usr/bin/kitty""#), "Run kitty");
        assert_eq!(describe(r#"spawn "playerctl" "play-pause""#), "Run playerctl play-pause");
        assert_eq!(describe("focus-workspace 3"), "Focus workspace 3");
        assert_eq!(describe("close-window"), "Close window");
    }

    #[test]
    fn renders_binds() {
        let spec = BindSpec {
            key: "Mod+T".into(),
            action: r#"spawn "kitty""#.into(),
            title: Some("Terminal".into()),
            repeat: false,
            allow_when_locked: false,
        };
        assert_eq!(spec.render(), r#"Mod+T hotkey-overlay-title="Terminal" repeat=false { spawn "kitty"; }"#);
        assert_eq!(key_name("Mod+Shift+Slash"), "Mod+Shift+Slash");
    }
}
