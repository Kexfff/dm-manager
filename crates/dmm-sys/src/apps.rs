//! The default terminal, which isn't a MIME type: `xdg-terminals.list` (the xdg-terminal-exec
//! spec) and KDE's `TerminalApplication`/`TerminalService`, which Dolphin's "Open Terminal"
//! uses. Other default apps are set through GIO in the app.

use std::process::{Command, Stdio};

use crate::ini::Ini;

fn terminals_list() -> std::path::PathBuf {
    crate::config_dir().join("xdg-terminals.list")
}

/// The default terminal's desktop file ID (`kitty.desktop`). With none set, KDE apps use
/// Konsole.
pub fn current_terminal() -> Option<String> {
    set_terminal_id().or_else(|| Some("org.kde.konsole.desktop".into()))
}

fn set_terminal_id() -> Option<String> {
    let kde = Ini::load(&crate::config_dir().join("kdeglobals"));
    if let Some(service) = kde.get("General", "TerminalService").filter(|s| !s.is_empty()) {
        return Some(if service.ends_with(".desktop") { service.to_string() } else { format!("{service}.desktop") });
    }
    let list = std::fs::read_to_string(terminals_list()).unwrap_or_default();
    list.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(String::from)
}

/// Makes `desktop_id` (running `program`) the default terminal.
pub fn set_terminal(desktop_id: &str, program: &str) -> Result<(), String> {
    for (key, value) in [("TerminalApplication", program), ("TerminalService", desktop_id)] {
        let ok = Command::new("kwriteconfig6")
            .args(["--file", "kdeglobals", "--group", "General", "--key", key, value])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            let path = crate::config_dir().join("kdeglobals");
            let mut ini = Ini::load(&path);
            ini.set("General", key, value);
            ini.save(&path)?;
        }
    }
    // The chosen one first; others stay as fallbacks.
    let path = terminals_list();
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines = vec![desktop_id.to_string()];
    lines.extend(old.lines().map(str::trim).filter(|l| !l.is_empty() && *l != desktop_id).map(String::from));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("Can't write {}: {e}", path.display()))
}

/// Has KDE rebuild its app database, so KDE apps see new default apps.
pub fn refresh_kde_cache() {
    let _ = Command::new("kbuildsycoca6").stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
