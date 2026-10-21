//! The wallpaper, through awww (which remembers it across restarts), and the folder of
//! wallpapers (`~/Pictures/Wallpapers`, which dm-desktop's picker lists too).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "avif", "jxl"];

/// `$XDG_PICTURES_DIR/Wallpapers`, else `~/Pictures/Wallpapers`.
pub fn folder() -> PathBuf {
    let home = crate::home().unwrap_or_default();
    let pictures = std::fs::read_to_string(crate::config_dir().join("user-dirs.dirs"))
        .ok()
        .and_then(|dirs| {
            dirs.lines().find_map(|l| {
                let value = l.strip_prefix("XDG_PICTURES_DIR=")?.trim().trim_matches('"');
                Some(PathBuf::from(value.replace("$HOME", &home.to_string_lossy())))
            })
        })
        .unwrap_or_else(|| home.join("Pictures"));
    pictures.join("Wallpapers")
}

/// The images in the folder, newest first.
pub fn library(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut images: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        })
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH), p))
        .collect();
    images.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    images.into_iter().map(|(_, p)| p).collect()
}

/// The images awww shows now (one per output).
pub fn current() -> Vec<PathBuf> {
    let Ok(out) = Command::new("awww").arg("query").output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once("image: ").map(|(_, p)| PathBuf::from(p.trim())))
        .collect()
}

/// Shows `image` on every output, with a fade. Starts awww's daemon if it isn't running.
pub fn set(image: &Path) -> Result<(), String> {
    let run = || {
        Command::new("awww")
            .arg("img")
            .arg(image)
            .args(["--transition-type", "fade", "--transition-duration", "1", "--transition-fps", "60"])
            .output()
            .map_err(|e| format!("Can't run awww: {e}"))
    };
    let out = run()?;
    if out.status.success() {
        return Ok(());
    }
    // Not running: start it (through niri, so it isn't this app's child) and try again.
    let _ = Command::new("niri").args(["msg", "action", "spawn", "--", "awww-daemon"]).status();
    std::thread::sleep(std::time::Duration::from_millis(700));
    let out = run()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("Error: ").to_string())
    }
}

/// A small JPEG of a local image for the grid, cached by path and modification time.
pub fn thumbnail_path(image: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    image.hash(&mut hasher);
    if let Ok(modified) = std::fs::metadata(image).and_then(|m| m.modified()) {
        modified.hash(&mut hasher);
    }
    crate::wallhaven::cache_dir().with_file_name("thumbnails").join(format!("{:016x}.png", hasher.finish()))
}
