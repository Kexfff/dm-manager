//! Editing INI-style files (`settings.ini`, `qt6ct.conf`, `kvantum.kvconfig`) line by line:
//! only the touched key changes; comments, order and everything else stay.

use std::path::Path;

pub struct Ini {
    lines: Vec<String>,
}

impl Ini {
    pub fn parse(text: &str) -> Self {
        Self { lines: text.lines().map(String::from).collect() }
    }

    /// Reads the file; a missing file is empty.
    pub fn load(path: &Path) -> Self {
        Self::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let range = self.section(section)?;
        self.lines[range].iter().find_map(|l| value_of(l, key))
    }

    /// Sets `key=value` in `[section]`, adding the key or the section as needed.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let line = format!("{key}={value}");
        let Some(range) = self.section(section) else {
            if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                self.lines.push(String::new());
            }
            self.lines.push(format!("[{section}]"));
            self.lines.push(line);
            return;
        };
        if let Some(i) = range.clone().find(|&i| value_of(&self.lines[i], key).is_some()) {
            self.lines[i] = line;
            return;
        }
        // After the section's last non-blank line.
        let mut at = range.end;
        while at > range.start && self.lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        self.lines.insert(at, line);
    }

    pub fn text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        }
        std::fs::write(path, self.text()).map_err(|e| format!("Can't write {}: {e}", path.display()))
    }

    /// The lines of `[section]`'s body.
    fn section(&self, name: &str) -> Option<std::ops::Range<usize>> {
        let header = format!("[{name}]");
        let start = self.lines.iter().position(|l| l.trim() == header)? + 1;
        let end = self.lines[start..]
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .map_or(self.lines.len(), |i| start + i);
        Some(start..end)
    }
}

fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once('=')?;
    (k.trim() == key).then(|| v.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str =
        "; comment\n[Settings]\ngtk-theme-name=adw-gtk3-dark\ngtk-font-name=Inter, 11\n\n[Other]\nkey = value\n";

    #[test]
    fn reads() {
        let ini = Ini::parse(TEXT);
        assert_eq!(ini.get("Settings", "gtk-theme-name"), Some("adw-gtk3-dark"));
        assert_eq!(ini.get("Other", "key"), Some("value"));
        assert_eq!(ini.get("Settings", "key"), None);
    }

    #[test]
    fn replaces_in_place() {
        let mut ini = Ini::parse(TEXT);
        ini.set("Settings", "gtk-theme-name", "Breeze");
        assert_eq!(ini.text(), TEXT.replace("adw-gtk3-dark", "Breeze"));
    }

    #[test]
    fn adds_keys_and_sections() {
        let mut ini = Ini::parse(TEXT);
        ini.set("Settings", "gtk-icon-theme-name", "Papirus");
        ini.set("New", "a", "1");
        assert_eq!(
            ini.text(),
            "; comment\n[Settings]\ngtk-theme-name=adw-gtk3-dark\ngtk-font-name=Inter, 11\ngtk-icon-theme-name=Papirus\n\n[Other]\nkey = value\n\n[New]\na=1\n"
        );
    }
}
