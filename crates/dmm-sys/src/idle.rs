//! Idle timers in swayidle's config (`~/.config/swayidle/config`), edited in place.
//!
//! Lines are recognised by what they run (turning screens off, locking, suspending). A timer
//! that's switched off is commented out with a marker rather than deleted, so switching it
//! back on restores the original command. swayidle doesn't reload its config, so a change
//! restarts it.

use std::path::{Path, PathBuf};
use std::process::Command;

const OFF: &str = "# dm-manager: off: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    ScreenOff,
    Lock,
    Suspend,
}

impl Timer {
    fn matches(self, line: &str) -> bool {
        match self {
            Self::ScreenOff => {
                line.contains("power-off-monitors") || line.contains("dpms") || line.contains("output * power off")
            }
            Self::Lock => {
                line.contains("lock-session")
                    || line.contains("swaylock")
                    || line.contains("hyprlock")
                    || line.contains("gtklock")
            }
            Self::Suspend => line.contains("suspend") || line.contains("hibernate") || line.contains("hybrid-sleep"),
        }
    }

    /// The line added when there's none to switch back on.
    fn default_line(self, seconds: u32) -> String {
        match self {
            Self::ScreenOff => {
                format!(
                    "timeout {seconds} 'niri msg action power-off-monitors' resume 'niri msg action power-on-monitors'"
                )
            }
            Self::Lock => format!("timeout {seconds} 'loginctl lock-session'"),
            Self::Suspend => format!("timeout {seconds} 'systemctl suspend'"),
        }
    }
}

pub struct IdleConfig {
    path: PathBuf,
    lines: Vec<String>,
}

impl IdleConfig {
    pub fn default_path() -> PathBuf {
        crate::config_dir().join("swayidle").join("config")
    }

    pub fn load(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self { path: path.to_path_buf(), lines: text.lines().map(String::from).collect() }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    /// Seconds before the timer fires, or `None` when it's off.
    pub fn timeout(&self, timer: Timer) -> Option<u32> {
        let i = self.find(timer, false)?;
        seconds(&self.lines[i])
    }

    pub fn set_timeout(&mut self, timer: Timer, seconds: Option<u32>) {
        match (seconds, self.find(timer, false)) {
            (Some(s), Some(i)) => self.lines[i] = with_seconds(&self.lines[i], s),
            (Some(s), None) => match self.find(timer, true) {
                Some(i) => self.lines[i] = with_seconds(&self.lines[i][OFF.len()..], s),
                None => {
                    // After the last timeout line, or at the end.
                    let at = self
                        .lines
                        .iter()
                        .rposition(|l| l.trim_start().starts_with("timeout"))
                        .map_or(self.lines.len(), |i| i + 1);
                    self.lines.insert(at, timer.default_line(s));
                }
            },
            (None, Some(i)) => self.lines[i] = format!("{OFF}{}", self.lines[i]),
            (None, None) => {}
        }
    }

    /// Locks the screen before the computer sleeps.
    pub fn lock_before_sleep(&self) -> bool {
        self.lines.iter().any(|l| l.trim_start().starts_with("before-sleep"))
    }

    pub fn set_lock_before_sleep(&mut self, on: bool) {
        let active = self.lines.iter().position(|l| l.trim_start().starts_with("before-sleep"));
        let off = self
            .lines
            .iter()
            .position(|l| l.starts_with(OFF) && l[OFF.len()..].trim_start().starts_with("before-sleep"));
        match (on, active, off) {
            (true, None, Some(i)) => self.lines[i] = self.lines[i][OFF.len()..].to_string(),
            (true, None, None) => self.lines.push("before-sleep 'loginctl lock-session'".into()),
            (false, Some(i), _) => self.lines[i] = format!("{OFF}{}", self.lines[i]),
            _ => {}
        }
    }

    /// The active (or switched-off) `timeout` line for this timer.
    fn find(&self, timer: Timer, switched_off: bool) -> Option<usize> {
        self.lines.iter().position(|l| {
            let line = match (switched_off, l.strip_prefix(OFF)) {
                (true, Some(rest)) => rest,
                (false, None) => l.as_str(),
                _ => return false,
            };
            line.trim_start().starts_with("timeout") && timer.matches(line)
        })
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, self.text()).map_err(|e| format!("Can't write {}: {e}", self.path.display()))
    }
}

/// The seconds of a `timeout <seconds> …` line.
fn seconds(line: &str) -> Option<u32> {
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The line with its seconds replaced, keeping everything else.
fn with_seconds(line: &str, seconds: u32) -> String {
    let start = line.find("timeout").map_or(0, |i| i + "timeout".len());
    let rest = &line[start..];
    let number_start = start + rest.len() - rest.trim_start().len();
    let number_end = line[number_start..].find(char::is_whitespace).map_or(line.len(), |i| number_start + i);
    format!("{}{seconds}{}", &line[..number_start], &line[number_end..])
}

pub fn is_running() -> bool {
    Command::new("pgrep").args(["-x", "swayidle"]).output().is_ok_and(|o| o.status.success())
}

/// Restarts swayidle with the new config (through niri, so it isn't this app's child).
pub fn restart() -> Result<(), String> {
    let _ = Command::new("pkill").args(["-x", "swayidle"]).status();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let out = Command::new("niri")
        .args(["msg", "action", "spawn", "--", "swayidle", "-w"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "# Screen off after 5 minutes, lock after 10.
timeout 300 'niri msg action power-off-monitors' resume 'niri msg action power-on-monitors'
timeout 600 'loginctl lock-session'
lock '~/.config/swayidle/lock'
before-sleep '~/.config/swayidle/lock'
";

    fn config(text: &str) -> IdleConfig {
        IdleConfig { path: PathBuf::from("/nonexistent"), lines: text.lines().map(String::from).collect() }
    }

    #[test]
    fn reads_timers() {
        let c = config(CONFIG);
        assert_eq!(c.timeout(Timer::ScreenOff), Some(300));
        assert_eq!(c.timeout(Timer::Lock), Some(600));
        assert_eq!(c.timeout(Timer::Suspend), None);
        assert!(c.lock_before_sleep());
    }

    #[test]
    fn changes_only_the_number() {
        let mut c = config(CONFIG);
        c.set_timeout(Timer::Lock, Some(900));
        assert_eq!(c.text(), CONFIG.replace("timeout 600", "timeout 900"));
    }

    #[test]
    fn switching_off_and_on_restores_the_line() {
        let mut c = config(CONFIG);
        c.set_timeout(Timer::ScreenOff, None);
        assert_eq!(c.timeout(Timer::ScreenOff), None);
        assert!(c.text().contains("# dm-manager: off: timeout 300 'niri msg action power-off-monitors'"));
        c.set_timeout(Timer::ScreenOff, Some(120));
        assert_eq!(c.text(), CONFIG.replace("timeout 300", "timeout 120"));
    }

    #[test]
    fn adds_a_suspend_timer_after_the_others() {
        let mut c = config(CONFIG);
        c.set_timeout(Timer::Suspend, Some(1800));
        let lines: Vec<String> = c.text().lines().map(String::from).collect();
        assert_eq!(lines[3], "timeout 1800 'systemctl suspend'");
        assert_eq!(c.timeout(Timer::Suspend), Some(1800));
        // The lock timer isn't mistaken for it, nor the other way round.
        assert_eq!(c.timeout(Timer::Lock), Some(600));
    }

    #[test]
    fn lock_before_sleep() {
        let mut c = config(CONFIG);
        c.set_lock_before_sleep(false);
        assert!(!c.lock_before_sleep());
        c.set_lock_before_sleep(true);
        assert_eq!(c.text(), CONFIG);
    }
}
