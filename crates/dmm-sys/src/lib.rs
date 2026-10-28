//! System backends for dm-manager: NetworkManager and BlueZ over D-Bus, and the config files
//! of dmsh and niri.
//!
//! No GTK here. The D-Bus backends are async `run` loops that take requests from a channel
//! and report state through a callback, so the UI only ever sees plain data.

pub mod appearance;
pub mod apps;
pub mod binds;
pub mod bluetooth;
mod bus;
pub mod dmsh;
pub mod idle;
pub mod ini;
pub mod kdl_text;
pub mod network;
pub mod niri;
pub mod outputs;
pub mod palette;
pub mod power;
pub mod sysinfo;
pub mod wallhaven;
pub mod wallpaper;

pub use bus::describe_error;

use std::path::PathBuf;

pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().unwrap_or_default().join(".config"))
}

/// `$XDG_DATA_DIRS`, else the usual `/usr/local/share:/usr/share` (on NixOS the variable
/// points into the Nix profiles).
pub fn data_dirs() -> Vec<PathBuf> {
    std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Directories Qt plugins may be in: `$QT_PLUGIN_PATH`, the usual system ones, and the
/// Nix profiles.
pub fn qt_plugin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var("QT_PLUGIN_PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    dirs.extend(
        ["/usr/lib/qt6/plugins", "/usr/lib64/qt6/plugins", "/usr/lib/x86_64-linux-gnu/qt6/plugins"].map(PathBuf::from),
    );
    let user = std::env::var("USER").unwrap_or_default();
    let mut profiles =
        vec![PathBuf::from("/run/current-system/sw"), PathBuf::from(format!("/etc/profiles/per-user/{user}"))];
    profiles.extend(home().map(|h| h.join(".nix-profile")));
    dirs.extend(profiles.into_iter().map(|p| p.join("lib/qt-6/plugins")));
    dirs
}
