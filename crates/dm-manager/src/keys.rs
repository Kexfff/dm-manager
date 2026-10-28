//! Key combinations between niri's names (`Mod+Shift+T`) and GTK (key events, and
//! accelerators for keycap labels).

use relm4::gtk::{gdk, prelude::*};

/// The niri name for a key press, or `None` for a lone modifier. Super counts as `Mod`
/// (niri's default mod key).
pub fn from_event(display: &gdk::Display, keyval: gdk::Key, keycode: u32, state: gdk::ModifierType) -> Option<String> {
    // The key without Shift or the layout's levels: Shift+1 is "1", not "exclam".
    let base = display.translate_key(keycode, gdk::ModifierType::empty(), 0).map_or(keyval, |(k, ..)| k);
    let name = base.name()?.to_string();
    if is_modifier(&name) {
        return None;
    }
    let mut parts = Vec::new();
    if state.contains(gdk::ModifierType::SUPER_MASK) || state.contains(gdk::ModifierType::META_MASK) {
        parts.push("Mod".to_string());
    }
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push("Ctrl".into());
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        parts.push("Alt".into());
    }
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push("Shift".into());
    }
    parts.push(key_case(&name));
    Some(parts.join("+"))
}

fn is_modifier(name: &str) -> bool {
    ["Shift_", "Control_", "Alt_", "Super_", "Meta_", "Hyper_", "ISO_Level3", "ISO_Level5", "Caps_Lock", "Num_Lock"]
        .iter()
        .any(|m| name.starts_with(m))
}

/// "t" → "T", "comma" → "Comma"; keysym names like "XF86AudioMute" stay.
fn key_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().collect::<String>() + chars.as_str(),
        _ => name.to_string(),
    }
}

/// The keycaps to show for a niri key: `Mod+Shift+Slash` → `["Super", "Shift", "/"]`.
pub fn caps(key: &str) -> Vec<String> {
    let parts: Vec<&str> = key.split('+').filter(|p| !p.is_empty()).collect();
    let Some((last, mods)) = parts.split_last() else { return Vec::new() };
    let mut caps: Vec<String> = mods
        .iter()
        .map(|m| match m.to_lowercase().as_str() {
            "mod" | "super" | "win" => "Super".to_string(),
            "ctrl" | "control" => "Ctrl".to_string(),
            other => key_case(other),
        })
        .collect();
    caps.push(key_label(last));
    caps
}

/// A key's label: its character if it types one ("/", "1", "T"), else a readable name.
fn key_label(name: &str) -> String {
    let keyval = gdk::Key::from_name(name).or_else(|| gdk::Key::from_name(name.to_lowercase()));
    if let Some(c) =
        keyval.and_then(|k| k.to_unicode()).filter(|c| c.is_ascii_graphic() || (!c.is_control() && !c.is_whitespace()))
    {
        return c.to_uppercase().collect();
    }
    let pretty = match name {
        "Return" => "Enter",
        "space" | "Space" => "Space",
        "Print" => "Print Screen",
        "Prior" | "Page_Up" => "Page Up",
        "Next" | "Page_Down" => "Page Down",
        "XF86AudioRaiseVolume" => "Volume Up",
        "XF86AudioLowerVolume" => "Volume Down",
        "XF86AudioMute" => "Mute",
        "XF86AudioMicMute" => "Mic Mute",
        "XF86AudioPlay" => "Play",
        "XF86AudioNext" => "Next Track",
        "XF86AudioPrev" => "Previous Track",
        "XF86MonBrightnessUp" => "Brightness Up",
        "XF86MonBrightnessDown" => "Brightness Down",
        other => return other.trim_start_matches("XF86").replace('_', " "),
    };
    pretty.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycaps() {
        assert_eq!(caps("Mod+Shift+T"), ["Super", "Shift", "T"]);
        assert_eq!(caps("Mod+Shift+Slash"), ["Super", "Shift", "/"]);
        assert_eq!(caps("Mod+Return"), ["Super", "Enter"]);
        assert_eq!(caps("XF86AudioRaiseVolume"), ["Volume Up"]);
        assert_eq!(caps("Mod+WheelScrollDown"), ["Super", "WheelScrollDown"]);
        assert_eq!(caps("Ctrl+Alt+Delete"), ["Ctrl", "Alt", "Delete"]);
    }

    #[test]
    fn key_names() {
        assert_eq!(key_case("t"), "T");
        assert_eq!(key_case("comma"), "Comma");
        assert_eq!(key_case("XF86AudioMute"), "XF86AudioMute");
        assert!(is_modifier("Shift_L") && is_modifier("Super_R") && !is_modifier("Return"));
    }
}
