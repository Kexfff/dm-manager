//! System backends for dm-manager: NetworkManager and BlueZ over D-Bus, and the config files
//! of dmsh and niri.
//!
//! No GTK here. The D-Bus backends are async `run` loops that take requests from a channel
//! and report state through a callback, so the UI only ever sees plain data.

pub mod appearance;
pub mod apps;
pub mod bluetooth;
mod bus;
pub mod dmsh;
pub mod idle;
pub mod ini;
pub mod kdl_text;
pub mod network;
pub mod niri;
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
