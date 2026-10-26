//! What this computer is: hardware and software, read from /proc, /sys, /etc and a few
//! tools (lspci, df, niri, dmsh). Nothing needs root.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disk {
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Display {
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemInfo {
    pub hostname: String,
    pub user: String,
    pub os_name: String,
    /// An icon name or file for the distribution's logo.
    pub os_logo: Option<String>,
    pub kernel: String,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub cpu: Option<String>,
    pub cores: usize,
    pub threads: usize,
    /// Installed memory in bytes (what the modules add up to, else what the kernel sees).
    pub memory: u64,
    /// "LPDDR5", "DDR4"…
    pub memory_type: Option<String>,
    pub gpus: Vec<String>,
    pub disk: Option<Disk>,
    pub displays: Vec<Display>,
    pub uptime: Duration,
    pub compositor: Option<String>,
    pub shell: Option<String>,
    pub packages: Option<usize>,
}

pub fn read() -> SystemInfo {
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let os_field = |key: &str| {
        os.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(|v| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    };
    let (cpu, cores, threads) = cpu();
    SystemInfo {
        hostname: read_trim("/proc/sys/kernel/hostname").unwrap_or_else(|| "localhost".into()),
        user: std::env::var("USER").unwrap_or_default(),
        os_name: os_field("PRETTY_NAME").or_else(|| os_field("NAME")).unwrap_or_else(|| "Linux".into()),
        os_logo: os_field("LOGO").or_else(|| os_field("ID")),
        kernel: read_trim("/proc/sys/kernel/osrelease").unwrap_or_default(),
        vendor: dmi("sys_vendor"),
        model: dmi("product_name").or_else(|| dmi("board_name")),
        cpu,
        cores,
        threads,
        memory: installed_memory().unwrap_or_else(meminfo_total),
        memory_type: dmi_property("MEMORY_DEVICE_0_TYPE")
            .filter(|t| !matches!(t.as_str(), "Unknown" | "Other" | "RAM")),
        gpus: gpus(),
        disk: disk("/"),
        displays: displays(),
        uptime: read_trim("/proc/uptime")
            .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
            .map(Duration::from_secs_f64)
            .unwrap_or_default(),
        compositor: version("niri"),
        shell: crate::dmsh::binary().and_then(|b| version(&b.to_string_lossy())),
        packages: package_count(),
    }
}

impl SystemInfo {
    /// "XIAOMI REDMI Book Pro 14 2025", without the vendor twice.
    pub fn device(&self) -> Option<String> {
        let model = self.model.clone()?;
        Some(match &self.vendor {
            Some(v) if !model.to_lowercase().starts_with(&v.to_lowercase()) && !generic(v) => {
                format!("{} {model}", title_case(v))
            }
            _ => model,
        })
    }

    /// A plain-text summary, for bug reports.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!("OS: {}", self.os_name), format!("Kernel: {}", self.kernel)];
        if let Some(d) = self.device() {
            lines.push(format!("Device: {d}"));
        }
        if let Some(cpu) = &self.cpu {
            lines.push(format!("CPU: {cpu} ({} cores, {} threads)", self.cores, self.threads));
        }
        lines.push(format!("Memory: {}", memory(self.memory)));
        for gpu in &self.gpus {
            lines.push(format!("GPU: {gpu}"));
        }
        for d in &self.displays {
            lines
                .push(format!("Display: {} {}×{} @ {:.0} Hz, scale {}", d.name, d.width, d.height, d.refresh, d.scale));
        }
        if let Some(c) = &self.compositor {
            lines.push(format!("Compositor: {c}"));
        }
        if let Some(s) = &self.shell {
            lines.push(format!("Shell: {s}"));
        }
        lines.join("\n")
    }
}

/// Firmware placeholders vendors leave in DMI fields.
fn generic(s: &str) -> bool {
    let s = s.to_lowercase();
    s.is_empty()
        || s.contains("to be filled")
        || s.contains("default string")
        || s == "system manufacturer"
        || s == "o.e.m."
}

/// "XIAOMI" → "Xiaomi"; mixed-case names stay.
fn title_case(s: &str) -> String {
    if s.chars().any(|c| c.is_lowercase()) {
        return s.to_string();
    }
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_string() + &c.as_str().to_lowercase()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "1.2 GB", "950 GB" (decimal units, like disk makers and file managers).
pub fn bytes(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 || unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// "3 days, 4 hours", "2 hours, 5 minutes", "12 minutes"
pub fn duration(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    let plural = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (days, hours) {
        (0, 0) => plural(minutes, "minute"),
        (0, h) => format!("{}, {}", plural(h, "hour"), plural(minutes, "minute")),
        (d, h) => format!("{}, {}", plural(d, "day"), plural(h, "hour")),
    }
}

fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn dmi(field: &str) -> Option<String> {
    read_trim(PathBuf::from("/sys/class/dmi/id").join(field)).filter(|v| !generic(v))
}

/// `(model, physical cores, logical CPUs)`
fn cpu() -> (Option<String>, usize, usize) {
    let info = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let model = info.lines().find_map(|l| l.strip_prefix("model name")?.split_once(':').map(|(_, v)| clean_cpu(v)));
    let threads = info.lines().filter(|l| l.starts_with("processor")).count().max(1);
    let mut cores: Vec<(String, String)> = Vec::new();
    let mut physical = String::new();
    for line in info.lines() {
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "physical id" => physical = value.trim().to_string(),
                "core id" => cores.push((physical.clone(), value.trim().to_string())),
                _ => {}
            }
        }
    }
    cores.sort();
    cores.dedup();
    (model, if cores.is_empty() { threads } else { cores.len() }, threads)
}

/// "Intel(R) Core(TM) Ultra 5 225H" → "Intel Core Ultra 5 225H"
fn clean_cpu(s: &str) -> String {
    let s = s.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
    let s = s.split(" @ ").next().unwrap_or(&s).replace(" CPU", "");
    let s = s.split(" with ").next().unwrap_or(&s).to_string();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn dmi_properties() -> String {
    Command::new("udevadm")
        .args(["info", "--query=property", "/sys/devices/virtual/dmi/id"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn dmi_property(key: &str) -> Option<String> {
    dmi_properties().lines().find_map(|l| l.strip_prefix(&format!("{key}=")).map(String::from))
}

/// The sizes of the memory modules, from the firmware tables udev keeps.
fn installed_memory() -> Option<u64> {
    let total: u64 = dmi_properties()
        .lines()
        .filter_map(|l| {
            // MEMORY_DEVICE_<n>_SIZE (not _VOLATILE_SIZE and the like)
            let (key, value) = l.split_once('=')?;
            let n = key.strip_prefix("MEMORY_DEVICE_")?.strip_suffix("_SIZE")?;
            n.bytes().all(|b| b.is_ascii_digit()).then(|| value.parse::<u64>().ok())?
        })
        .sum();
    (total > 0).then_some(total)
}

/// Memory as people count it: binary units ("32 GB" for 32 GiB of modules).
pub fn memory(n: u64) -> String {
    let gib = n as f64 / (1u64 << 30) as f64;
    if gib.fract() < 0.05 { format!("{gib:.0} GB") } else { format!("{gib:.1} GB") }
}

fn meminfo_total() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|m| {
            m.lines().find_map(|l| l.strip_prefix("MemTotal:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())
        })
        .map_or(0, |kb| kb * 1024)
}

fn gpus() -> Vec<String> {
    let Ok(out) = Command::new("lspci").arg("-mm").output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).lines().filter_map(gpu_from_lspci).collect()
}

/// A display controller line of `lspci -mm` as "Vendor Model".
fn gpu_from_lspci(line: &str) -> Option<String> {
    // slot "class" "vendor" "device" …
    let fields: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    let (class, vendor, device) = (fields.first()?, fields.get(1)?, fields.get(2)?);
    if !(class.contains("VGA") || class.contains("3D") || class.contains("Display")) {
        return None;
    }
    let vendor = if vendor.contains("Intel") {
        "Intel"
    } else if vendor.contains("AMD") || vendor.contains("ATI") {
        "AMD"
    } else if vendor.contains("NVIDIA") {
        "NVIDIA"
    } else {
        // "Red Hat, Inc." → "Red Hat"
        let name = vendor.split(',').next().unwrap_or(vendor).trim();
        ["Corporation", "Corp.", "Inc.", "Ltd.", "Co."].iter().fold(name, |n, s| n.trim_end_matches(s).trim())
    };
    // Marketing names are in brackets: "Arrow Lake-P [Arc Pro 130T/140T]".
    let model = match (device.rfind('['), device.rfind(']')) {
        (Some(a), Some(b)) if b > a => &device[a + 1..b],
        _ => device,
    };
    Some(format!("{vendor} {model}"))
}

fn disk(mount: &str) -> Option<Disk> {
    let out = Command::new("df").args(["-B1", "--output=size,used", mount]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut numbers = text.lines().nth(1)?.split_whitespace().map(|n| n.parse::<u64>());
    Some(Disk { total: numbers.next()?.ok()?, used: numbers.next()?.ok()? })
}

/// Connected displays with their current mode, from niri.
pub fn displays() -> Vec<Display> {
    let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() else { return Vec::new() };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return Vec::new() };
    let mut displays: Vec<Display> = json
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, o)| {
            let mode = o.get("modes")?.get(o.get("current_mode")?.as_u64()? as usize)?;
            let text = |k: &str| o.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
            Some(Display {
                name: name.clone(),
                description: format!("{} {}", text("make"), text("model")).trim().to_string(),
                width: mode.get("width")?.as_u64()? as u32,
                height: mode.get("height")?.as_u64()? as u32,
                refresh: mode.get("refresh_rate")?.as_f64()? / 1000.0,
                scale: o.get("logical").and_then(|l| l.get("scale")).and_then(|s| s.as_f64()).unwrap_or(1.0),
            })
        })
        .collect();
    displays.sort_by(|a, b| a.name.cmp(&b.name));
    displays
}

/// The first line of `program --version`.
fn version(program: &str) -> Option<String> {
    let out = Command::new(program).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn package_count() -> Option<usize> {
    // NixOS: the store paths of the system (what neofetch and friends count).
    if Path::new("/run/current-system/sw").exists()
        && let Ok(out) = Command::new("nix-store").args(["--query", "--requisites", "/run/current-system/sw"]).output()
        && out.status.success()
    {
        return Some(out.stdout.iter().filter(|b| **b == b'\n').count());
    }
    for (program, args) in [("pacman", &["-Qq"][..]), ("dpkg-query", &["-f", ".\n", "-W"][..]), ("rpm", &["-qa"][..])] {
        if let Ok(out) = Command::new(program).args(args).output()
            && out.status.success()
        {
            return Some(out.stdout.iter().filter(|b| **b == b'\n').count());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_names() {
        assert_eq!(clean_cpu(" Intel(R) Core(TM) Ultra 5 225H"), "Intel Core Ultra 5 225H");
        assert_eq!(clean_cpu("Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz"), "Intel Core i7-8550U");
        assert_eq!(clean_cpu("AMD Ryzen 7 7840U w/ Radeon 780M Graphics"), "AMD Ryzen 7 7840U w/ Radeon 780M Graphics");
    }

    #[test]
    fn gpu_names() {
        let line = r#"00:02.0 "VGA compatible controller" "Intel Corporation" "Arrow Lake-P [Arc Pro 130T/140T]" -r03 -p00 "Xiaomi" "Device 2411""#;
        assert_eq!(gpu_from_lspci(line).as_deref(), Some("Intel Arc Pro 130T/140T"));
        let line = r#"01:00.0 "3D controller" "NVIDIA Corporation" "AD107M [GeForce RTX 4060 Max-Q / Mobile]" "" """#;
        assert_eq!(gpu_from_lspci(line).as_deref(), Some("NVIDIA GeForce RTX 4060 Max-Q / Mobile"));
        assert_eq!(gpu_from_lspci(r#"00:14.0 "USB controller" "Intel Corporation" "x""#), None);
        let line = r#"00:01.0 "VGA compatible controller" "Red Hat, Inc." "Virtio 1.0 GPU" -r01 "Red Hat, Inc." "Device 1100""#;
        assert_eq!(gpu_from_lspci(line).as_deref(), Some("Red Hat Virtio 1.0 GPU"));
    }

    #[test]
    fn formatting() {
        assert_eq!(memory(32 << 30), "32 GB");
        assert_eq!(memory(32_267_784 * 1024), "30.8 GB");
        assert_eq!(bytes(950_000_000_000), "950 GB");
        assert_eq!(duration(Duration::from_secs(5 * 60)), "5 minutes");
        assert_eq!(duration(Duration::from_secs(3600 + 60)), "1 hour, 1 minute");
        assert_eq!(duration(Duration::from_secs(3 * 86400 + 4 * 3600)), "3 days, 4 hours");
        assert_eq!(title_case("XIAOMI"), "Xiaomi");
    }
}
