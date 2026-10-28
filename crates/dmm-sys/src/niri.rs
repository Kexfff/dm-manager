//! niri's keyboard, pointer and cursor settings.
//!
//! dm-manager writes only its own file, `~/.config/niri/dm-manager.kdl`, which the main
//! config includes last, so its values win. To show what's in effect it reads the whole
//! config, following includes and niri's merge rules: most sections merge setting by
//! setting, but pointing devices (`touchpad`, `mouse`) are replaced as a whole block.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::kdl_text::{KdlText, Value};

pub const FILE_NAME: &str = "dm-manager.kdl";

const HEADER: &str = "// Settings made in DM Manager (https://github.com/Kexfff/dm-manager).
// niri's config.kdl includes this file last, so these win over the files before it.
// The app rewrites the values it manages; anything else you add here is kept.
";

const INCLUDE: &str = "
// Settings made in DM Manager. Keep this last so they win over the files above.
include optional=true \"dm-manager.kdl\"
";

/// Where a setting lives in the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Keyboard,
    Xkb,
    Touchpad,
    Mouse,
    Cursor,
    Environment,
}

impl Area {
    fn path(self) -> &'static [&'static str] {
        match self {
            Self::Keyboard => &["input", "keyboard"],
            Self::Xkb => &["input", "keyboard", "xkb"],
            Self::Touchpad => &["input", "touchpad"],
            Self::Mouse => &["input", "mouse"],
            Self::Cursor => &["cursor"],
            Self::Environment => &["environment"],
        }
    }

    /// niri replaces these sections as a whole instead of merging them.
    fn replaced_whole(self) -> bool {
        matches!(self, Self::Touchpad | Self::Mouse)
    }
}

/// The settings of one section, as in effect. Flags read as `Bool`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub fields: BTreeMap<String, Value>,
    /// For sections replaced as a whole: the text of the block in effect.
    raw: Option<String>,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == Some(&Value::Bool(true))
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
}

/// A key bind as written in its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Bind {
    /// The key combination, as niri writes it (`Mod+Shift+T`).
    pub key: String,
    /// The action as written (`spawn "kitty"`, `focus-workspace 3`).
    pub action: String,
    /// The program it starts, for `spawn` binds.
    pub spawn: Option<String>,
    /// `hotkey-overlay-title`.
    pub title: Option<String>,
    /// `hotkey-overlay-title=null`: hidden from the hotkey overlay.
    pub hidden: bool,
    pub repeat: bool,
    pub allow_when_locked: bool,
    pub cooldown_ms: Option<u32>,
    pub allow_inhibiting: bool,
    /// The file it's in.
    pub source: PathBuf,
    /// It's in dm-manager's own file (so dm-manager may change it).
    pub own: bool,
    /// The bind it replaces from an earlier file: `(file, action)`.
    pub overrides: Option<(PathBuf, String)>,
    raw: String,
    /// Where the action node is in `raw`.
    action_span: (usize, usize),
}

/// An `output "…" { … }` block in effect (niri uses the first one for each output).
#[derive(Debug, Clone, PartialEq)]
pub struct OutputBlock {
    /// The name it matches: a connector (`eDP-1`) or "Make Model Serial".
    pub name: String,
    pub source: PathBuf,
    pub own: bool,
    raw: String,
}

/// The input and cursor settings in effect, read from the whole niri config.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputConfig {
    pub keyboard: Section,
    pub xkb: Section,
    pub touchpad: Section,
    pub mouse: Section,
    pub cursor: Section,
    /// Environment variables niri sets for what it starts (a null value unsets one).
    pub environment: Section,
    /// Key binds in effect, by key (`Mod+T`).
    pub binds: BTreeMap<String, Bind>,
    /// Output blocks in effect, in order (the first for a name applies).
    pub outputs: Vec<OutputBlock>,
    /// The main config includes dm-manager's file.
    pub included: bool,
    /// The main config couldn't be read; nothing above is reliable.
    pub error: Option<String>,
    /// There's no niri config at all (niri writes one when it first starts).
    pub missing: bool,
}

impl InputConfig {
    pub fn section(&self, area: Area) -> &Section {
        match area {
            Area::Keyboard => &self.keyboard,
            Area::Xkb => &self.xkb,
            Area::Touchpad => &self.touchpad,
            Area::Mouse => &self.mouse,
            Area::Cursor => &self.cursor,
            Area::Environment => &self.environment,
        }
    }

    /// Keyboard layouts as `(layout, variant)` pairs, in switching order.
    pub fn layouts(&self) -> Vec<(String, String)> {
        let layouts = self.xkb.string("layout").unwrap_or_default();
        let variants: Vec<&str> = self.xkb.string("variant").unwrap_or_default().split(',').collect();
        layouts
            .split(',')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .enumerate()
            .map(|(i, l)| (l.to_string(), variants.get(i).map_or("", |v| v.trim()).to_string()))
            .collect()
    }
}

/// `layout` and `variant` values for these `(layout, variant)` pairs; no variant value when
/// none has one.
pub fn join_layouts(layouts: &[(String, String)]) -> (String, Option<String>) {
    let layout = layouts.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>().join(",");
    let variants = layouts.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(",");
    let any = layouts.iter().any(|(_, v)| !v.is_empty());
    (layout, any.then_some(variants))
}

/// The system's keyboard layouts (`localectl`), which niri uses when its config sets none.
pub fn system_layouts() -> Vec<(String, String)> {
    let Ok(out) = Command::new("localectl").arg("status").output() else { return Vec::new() };
    parse_localectl(&String::from_utf8_lossy(&out.stdout))
}

fn parse_localectl(status: &str) -> Vec<(String, String)> {
    let field = |name: &str| {
        status.lines().find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_string())).unwrap_or_default()
    };
    let variants = field("X11 Variant:");
    let variants: Vec<&str> = variants.split(',').collect();
    field("X11 Layout:")
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, l)| (l.to_string(), variants.get(i).map_or("", |v| v.trim()).to_string()))
        .collect()
}

/// The `grp:` option (how layouts are switched) in an xkb options string.
pub fn switch_option(options: &str) -> Option<&str> {
    options.split(',').map(str::trim).find(|o| o.starts_with("grp:"))
}

/// The options string with its `grp:` option replaced (or removed); `None` if nothing's left.
pub fn with_switch_option(options: &str, switch: Option<&str>) -> Option<String> {
    let mut kept: Vec<&str> =
        options.split(',').map(str::trim).filter(|o| !o.is_empty() && !o.starts_with("grp:")).collect();
    kept.extend(switch);
    (!kept.is_empty()).then(|| kept.join(","))
}

pub fn niri_dir() -> PathBuf {
    crate::config_dir().join("niri")
}

/// niri's main config: `$NIRI_CONFIG`, else `~/.config/niri/config.kdl`.
/// niri's main config, as niri looks for it: `$NIRI_CONFIG`, `~/.config/niri/config.kdl`, then
/// `/etc/niri/config.kdl`.
pub fn main_config() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRI_CONFIG").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()) {
        return path;
    }
    let user = user_config();
    let system = PathBuf::from("/etc/niri/config.kdl");
    if !user.exists() && system.exists() { system } else { user }
}

/// Where the user's own niri config goes (and where dm-manager adds its include).
pub fn user_config() -> PathBuf {
    std::env::var_os("NIRI_CONFIG")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| niri_dir().join("config.kdl"))
}

/// A config to start the user's from, when they have none: the system's, or niri's default
/// (some distributions ship it as documentation).
fn config_template() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/etc/niri/config.kdl")];
    candidates.extend(crate::data_dirs().into_iter().map(|d| d.join("doc/niri/default-config.kdl")));
    candidates.into_iter().find(|p| p.exists())
}

/// Whether the include can be added: the user has a config, or one can be copied for them.
pub fn can_add_include() -> bool {
    user_config().exists() || config_template().is_some()
}

pub fn own_file() -> PathBuf {
    niri_dir().join(FILE_NAME)
}

/// Reads the settings in effect from the main config and everything it includes.
pub fn effective() -> InputConfig {
    let mut config = InputConfig::default();
    let main = main_config();
    if !main.exists() {
        config.missing = true;
    } else if let Err(e) = read_into(&main, &own_file(), &mut config, 0) {
        config.error = Some(e);
    }
    config
}

fn read_into(path: &Path, own: &Path, config: &mut InputConfig, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("includes nest too deeply".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("Can't read {}: {e}", path.display()))?;
    let file = KdlText::parse(text).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("/"));

    for node in file.document().nodes() {
        match node.name().value() {
            "include" => {
                let Some(target) = node.entries().iter().find(|e| e.name().is_none()) else {
                    continue;
                };
                let Some(target) = target.value().as_string() else {
                    continue;
                };
                let target = resolve(dir, target);
                let optional = node.entry("optional").is_some_and(|e| e.value().as_bool() == Some(true));
                if same_file(&target, own) {
                    config.included = true;
                }
                if let Err(e) = read_into(&target, own, config, depth + 1)
                    && !(optional && !target.exists())
                {
                    log::warn!("niri include: {e}");
                }
            }
            "input" => {
                for child in node.children().map(|c| c.nodes()).unwrap_or_default() {
                    match child.name().value() {
                        "keyboard" => {
                            merge(&mut config.keyboard, child);
                            if let Some(xkb) = child.children().and_then(|c| c.get("xkb")) {
                                merge(&mut config.xkb, xkb);
                            }
                        }
                        "touchpad" => config.touchpad = replace(child, file.text()),
                        "mouse" => config.mouse = replace(child, file.text()),
                        _ => {}
                    }
                }
            }
            "cursor" => merge(&mut config.cursor, node),
            "environment" => merge(&mut config.environment, node),
            "binds" => {
                let own = same_file(path, own);
                for bind in node.children().map(|c| c.nodes()).unwrap_or_default() {
                    let Some(action) = bind.children().and_then(|c| c.nodes().first()) else { continue };
                    let (start, end) = (bind.span().offset(), bind.span().offset() + bind.span().len());
                    let (a_start, a_end) = (action.span().offset(), action.span().offset() + action.span().len());
                    let key = bind.name().value().to_string();
                    let prop = |name: &str| bind.entry(name).map(|e| e.value());
                    let flag = |name: &str, default: bool| prop(name).and_then(|v| v.as_bool()).unwrap_or(default);
                    let spawn = (action.name().value() == "spawn")
                        .then(|| action.entries().first().and_then(|e| e.value().as_string().map(String::from)))
                        .flatten();
                    let overrides = config.binds.get(&key).map(|prev| {
                        prev.overrides.clone().unwrap_or_else(|| (prev.source.clone(), prev.action.clone()))
                    });
                    let new = Bind {
                        action: file.text()[a_start..a_end].trim_end_matches(';').trim().to_string(),
                        spawn,
                        title: prop("hotkey-overlay-title").and_then(|v| v.as_string()).map(String::from),
                        hidden: prop("hotkey-overlay-title").is_some_and(|v| v.is_null()),
                        repeat: flag("repeat", true),
                        allow_when_locked: flag("allow-when-locked", false),
                        cooldown_ms: prop("cooldown-ms")
                            .and_then(|v| v.as_integer())
                            .and_then(|n| u32::try_from(n).ok()),
                        allow_inhibiting: flag("allow-inhibiting", true),
                        source: path.to_path_buf(),
                        own,
                        overrides: overrides.filter(|(source, _)| !same_file(source, path)),
                        raw: file.text()[start..end].to_string(),
                        action_span: (a_start - start, a_end - start),
                        key: key.clone(),
                    };
                    config.binds.insert(key, new);
                }
            }
            "output" => {
                let Some(name) = node.entries().iter().find(|e| e.name().is_none()).and_then(|e| e.value().as_string())
                else {
                    continue;
                };
                // niri applies the first block for an output; later ones are ignored.
                if config.outputs.iter().any(|o| o.name.eq_ignore_ascii_case(name)) {
                    continue;
                }
                config.outputs.push(OutputBlock {
                    name: name.to_string(),
                    source: path.to_path_buf(),
                    own: same_file(path, own),
                    raw: file.node_text(node).to_string(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve(dir: &Path, target: &str) -> PathBuf {
    match target.strip_prefix("~/") {
        Some(rest) => crate::home().unwrap_or_default().join(rest),
        None => dir.join(target),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A setting's value: its argument, or `true` for a bare flag.
fn node_value(node: &kdl::KdlNode) -> Option<Value> {
    match node.entries().iter().find(|e| e.name().is_none()) {
        None => Some(Value::Bool(true)),
        Some(e) => Value::from_kdl(e.value()),
    }
}

/// Merges a section's settings over what's there (subsections aren't settings).
fn merge(section: &mut Section, node: &kdl::KdlNode) {
    for child in node.children().map(|c| c.nodes()).unwrap_or_default() {
        if child.children().is_some() {
            continue;
        }
        if let Some(value) = node_value(child) {
            section.fields.insert(child.name().value().to_string(), value);
        }
    }
}

fn replace(node: &kdl::KdlNode, text: &str) -> Section {
    let mut section = Section::default();
    merge(&mut section, node);
    let span = node.span();
    section.raw = Some(text[span.offset()..span.offset() + span.len()].to_string());
    section
}

/// dm-manager's own niri file.
pub struct NiriFile {
    path: PathBuf,
    file: KdlText,
}

impl NiriFile {
    pub fn load() -> Result<Self, String> {
        let path = own_file();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HEADER.to_string(),
            Err(e) => return Err(format!("Can't read {}: {e}", path.display())),
        };
        let file = KdlText::parse_v1(text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self { path, file })
    }

    pub fn text(&self) -> &str {
        self.file.text()
    }

    /// Sets a value (`None` goes back to what the other files say, or niri's default).
    /// `effective` is the config in effect, so blocks niri replaces as a whole can be
    /// carried over before one of their settings changes.
    pub fn set(&mut self, effective: &InputConfig, area: Area, key: &str, value: Option<Value>) -> Result<(), String> {
        let path = area.path();
        if area.replaced_whole() && self.file.node(path).is_none() {
            // This block will replace the one in effect; start from a copy of it.
            if let Some(raw) = &effective.section(area).raw {
                self.file.insert_node_text(&path[..path.len() - 1], raw)?;
            }
        }
        if area == Area::Xkb {
            // Write all of xkb, so it's right whether niri merges it or replaces it.
            for (k, v) in &effective.xkb.fields {
                if self.file.node(&[path, &[k.as_str()]].concat()).is_none() {
                    self.file.set(&[path, &[k.as_str()]].concat(), None, v)?;
                }
            }
        }

        let full: Vec<&str> = path.iter().copied().chain([key]).collect();
        match value {
            Some(Value::Bool(true)) => {
                // A flag: bare means on (a `false` argument from before would turn it off).
                self.file.remove(&full)?;
                self.file.set_flag(&full)
            }
            // In a block of our own, absent is off; elsewhere an earlier `true` needs a `false`.
            Some(Value::Bool(false)) if area.replaced_whole() => self.file.remove(&full),
            Some(v) => self.file.set(&full, None, &v),
            None => self.file.remove(&full),
        }
    }

    /// Makes the bind for `key` start `program` instead, keeping its key and options.
    pub fn rebind_spawn(&mut self, key: &str, bind: &Bind, program: &str) -> Result<(), String> {
        let action = format!("spawn {}", Value::Str(program.into()).repr(crate::kdl_text::Version::V1));
        let raw = format!("{}{action}{}", &bind.raw[..bind.action_span.0], &bind.raw[bind.action_span.1..]);
        self.file.remove(&["binds", key])?;
        self.file.insert_node_text(&["binds"], &raw)
    }

    /// Adds or replaces a bind (in this file only). `old_key` is the key it had, when an edit
    /// changes it.
    pub fn set_bind(&mut self, old_key: Option<&str>, spec: &crate::binds::BindSpec) -> Result<(), String> {
        if let Some(old) = old_key {
            self.remove_bind(old)?;
        }
        self.remove_bind(&spec.key)?;
        self.file.insert_node_text(&["binds"], &spec.render())
    }

    /// Removes a bind from this file (binds from other files aren't touched).
    pub fn remove_bind(&mut self, key: &str) -> Result<(), String> {
        self.file.remove(&["binds", key])
    }

    /// Writes an output's settings, replacing this file's block for it; children dm-manager
    /// doesn't manage (e.g. `focus-at-startup`) are kept.
    pub fn set_output(&mut self, name: &str, settings: &crate::outputs::Settings) -> Result<(), String> {
        let extra: Vec<String> = self
            .file
            .node_with_arg("output", name)
            .and_then(|n| n.children())
            .map(|c| {
                c.nodes()
                    .iter()
                    .filter(|n| !crate::outputs::MANAGED.contains(&n.name().value()))
                    .map(|n| self.file.node_text(n).to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.file.remove_with_arg("output", name)?;
        self.file.insert_node_text(&[], &crate::outputs::render_block(name, settings, &extra))
    }

    /// Checks the file with `niri validate`, then replaces it (niri reloads it on its own).
    pub fn save(&self) -> Result<(), String> {
        let dir = self.path.parent().ok_or("no niri config directory")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        let tmp = dir.join(".dm-manager.kdl.tmp");
        std::fs::write(&tmp, self.file.text()).map_err(|e| format!("Can't write {}: {e}", tmp.display()))?;
        match Command::new("niri").arg("validate").arg("-c").arg(&tmp).output() {
            Ok(out) if !out.status.success() => {
                let _ = std::fs::remove_file(&tmp);
                return Err(validate_error(&String::from_utf8_lossy(&out.stderr)));
            }
            Ok(_) => {}
            Err(e) => log::warn!("can't run niri validate: {e}"),
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("Can't save {}: {e}", self.path.display()))
    }
}

/// The useful line of `niri validate`'s report.
fn validate_error(stderr: &str) -> String {
    stderr
        .lines()
        .map(|l| l.trim().trim_start_matches(['×', '├', '╰', '─', '▶', ' ']))
        .rfind(|l| !l.is_empty() && !l.contains("error loading config") && !l.contains("error parsing"))
        .unwrap_or("niri rejected the settings")
        .to_string()
}

const MOVED: &str = "// Moved to dm-manager.kdl by DM Manager, which manages this output now:";

/// Moves an output's block from the user's file where niri reads it into dm-manager's file
/// (once, on request): the original is commented out with a note, its settings copied over.
/// Checked with `niri validate` on the whole config; on failure both files are put back.
pub fn manage_output(config: &InputConfig, name: &str) -> Result<(), String> {
    let block = config
        .outputs
        .iter()
        .find(|o| o.name.eq_ignore_ascii_case(name) && !o.own)
        .ok_or_else(|| format!("{name} isn't configured in another file"))?;
    let source_text =
        std::fs::read_to_string(&block.source).map_err(|e| format!("Can't read {}: {e}", block.source.display()))?;
    let commented = comment_out(&source_text, &block.raw)
        .ok_or_else(|| format!("Can't find {name}'s block in {}", block.source.display()))?;

    let mut own = NiriFile::load()?;
    own.file.remove_with_arg("output", &block.name)?;
    own.file.insert_node_text(&[], &block.raw)?;

    let own_before = std::fs::read_to_string(&own.path).ok();
    std::fs::write(&block.source, &commented).map_err(|e| format!("Can't write {}: {e}", block.source.display()))?;
    let checked = own.save().and_then(|()| validate(&main_config()));
    if let Err(e) = checked {
        let _ = std::fs::write(&block.source, &source_text);
        match own_before {
            Some(text) => {
                let _ = std::fs::write(&own.path, text);
            }
            None => {
                let _ = std::fs::remove_file(&own.path);
            }
        }
        return Err(e);
    }
    Ok(())
}

/// The text with the lines of `block` commented out, under a note.
fn comment_out(text: &str, block: &str) -> Option<String> {
    let start = text.find(block)?;
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let end = start + block.len();
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let indent: String = text[line_start..start].chars().take_while(|c| c.is_whitespace()).collect();
    let commented: Vec<String> = text[line_start..line_end]
        .lines()
        .map(|l| format!("{indent}// {}", l.strip_prefix(&indent).unwrap_or(l)))
        .collect();
    Some(format!("{}{indent}{MOVED}\n{}{}", &text[..line_start], commented.join("\n"), &text[line_end..]))
}

/// `niri validate` on a config file.
fn validate(path: &Path) -> Result<(), String> {
    match Command::new("niri").arg("validate").arg("-c").arg(path).output() {
        Ok(out) if !out.status.success() => Err(validate_error(&String::from_utf8_lossy(&out.stderr))),
        _ => Ok(()),
    }
}

/// Adds the include for dm-manager's file to the end of niri's main config.
pub fn add_include() -> Result<(), String> {
    let main = user_config();
    if !main.exists() {
        // Start from the system's or niri's default config: a file with only the include
        // would replace niri's defaults (keybinds and all).
        let template = config_template()
            .ok_or("niri hasn't written its config yet. Start niri once (it creates ~/.config/niri/config.kdl), then try again.")?;
        if let Some(dir) = main.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        }
        // Copy the text, not the file: the template may be read-only (the Nix store).
        let text = std::fs::read_to_string(&template).map_err(|e| format!("Can't read {}: {e}", template.display()))?;
        std::fs::write(&main, text).map_err(|e| format!("Can't write {}: {e}", main.display()))?;
    }
    let mut text = std::fs::read_to_string(&main).map_err(|e| format!("Can't read {}: {e}", main.display()))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(INCLUDE);
    std::fs::write(&main, text).map_err(|e| format!("Can't write {}: {e}", main.display()))
}

#[derive(Debug, Clone)]
pub struct XkbLayout {
    pub name: String,
    pub description: String,
    /// `(name, description)`
    pub variants: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct XkbOption {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct XkbCatalog {
    pub layouts: Vec<XkbLayout>,
    /// Ways to switch layouts (the `grp:` options).
    pub switch_options: Vec<XkbOption>,
}

impl XkbCatalog {
    /// "Russian (phonetic)", or the raw names if unknown.
    pub fn describe(&self, layout: &str, variant: &str) -> String {
        let Some(l) = self.layouts.iter().find(|l| l.name == layout) else {
            return if variant.is_empty() { layout.to_string() } else { format!("{layout} ({variant})") };
        };
        if variant.is_empty() {
            return l.description.clone();
        }
        l.variants
            .iter()
            .find(|(name, _)| name == variant)
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| format!("{} ({variant})", l.description))
    }
}

/// Layouts and switching options from xkeyboard-config's rules.
pub fn xkb_catalog() -> XkbCatalog {
    // $XKB_CONFIG_ROOT (set on NixOS), /etc/X11/xkb, then the data dirs.
    let mut roots: Vec<PathBuf> = std::env::var_os("XKB_CONFIG_ROOT").map(PathBuf::from).into_iter().collect();
    roots.push(PathBuf::from("/etc/X11/xkb"));
    for dir in crate::data_dirs() {
        roots.push(dir.join("X11/xkb"));
        roots.push(dir.join("xkeyboard-config-2"));
    }
    let path = roots.into_iter().map(|r| r.join("rules/evdev.xml")).find(|p| p.exists());
    let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else {
        return XkbCatalog::default();
    };
    parse_xkb_rules(&text)
}

fn parse_xkb_rules(xml: &str) -> XkbCatalog {
    // evdev.xml starts with a DOCTYPE, which roxmltree refuses unless told otherwise.
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, options) else {
        return XkbCatalog::default();
    };
    let mut catalog = XkbCatalog::default();
    for layout in doc.descendants().filter(|n| n.has_tag_name("layout")) {
        let Some((name, description)) = item(layout) else {
            continue;
        };
        let variants = child(layout, "variantList")
            .map(|list| list.children().filter(|n| n.has_tag_name("variant")).filter_map(item).collect())
            .unwrap_or_default();
        catalog.layouts.push(XkbLayout { name, description, variants });
    }
    catalog.layouts.sort_by_key(|l| l.description.to_lowercase());

    if let Some(group) =
        doc.descendants().filter(|n| n.has_tag_name("group")).find(|g| item(*g).is_some_and(|(name, _)| name == "grp"))
    {
        catalog.switch_options = group
            .children()
            .filter(|n| n.has_tag_name("option"))
            .filter_map(item)
            .map(|(name, description)| XkbOption { name, description })
            .collect();
    }
    catalog
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(tag))
}

/// A `configItem`'s `(name, description)`.
fn item(n: roxmltree::Node<'_, '_>) -> Option<(String, String)> {
    let config = child(n, "configItem")?;
    let name = child(config, "name")?.text()?.to_string();
    let description = child(config, "description").and_then(|d| d.text()).unwrap_or(&name).to_string();
    Some((name, description))
}

/// A connected display: `(connector, "Make Model")`, from `niri msg outputs`.
pub fn outputs() -> Vec<(String, String)> {
    let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() else { return Vec::new() };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return Vec::new() };
    let mut outputs: Vec<(String, String)> = json
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, o)| {
            let field = |k: &str| o.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string();
            let description = format!("{} {}", field("make"), field("model")).trim().to_string();
            (name.clone(), description)
        })
        .collect();
    outputs.sort();
    outputs
}

/// Whether a touchpad is connected.
pub fn has_touchpad() -> bool {
    std::fs::read_to_string("/proc/bus/input/devices")
        .is_ok_and(|d| d.lines().any(|l| l.starts_with("N: Name=") && l.to_lowercase().contains("touchpad")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorTheme {
    /// The directory name, which is what configs use.
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
}

/// Installed cursor themes (icon themes with a `cursors` directory).
pub fn cursor_themes() -> Vec<CursorTheme> {
    let home = crate::home().unwrap_or_default();
    let mut roots = vec![home.join(".local/share/icons"), home.join(".icons")];
    roots.extend(crate::data_dirs().into_iter().map(|d| d.join("icons")));
    roots.push(PathBuf::from("/usr/share/pixmaps"));

    let mut themes: Vec<CursorTheme> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            if !dir.join("cursors").is_dir() || themes.iter().any(|t| t.id == id) {
                continue;
            }
            let name = std::fs::read_to_string(dir.join("index.theme"))
                .ok()
                .and_then(|t| t.lines().find_map(|l| l.strip_prefix("Name=").map(|n| n.trim().to_string())))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| id.clone());
            themes.push(CursorTheme { id, name, dir });
        }
    }
    themes.sort_by_key(|t| t.name.to_lowercase());
    themes
}

/// One cursor image, as premultiplied BGRA bytes (what GDK calls B8G8R8A8_PREMULTIPLIED).
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The first of `names` the theme has, at the nominal size closest to `size`.
pub fn cursor_image(theme: &CursorTheme, names: &[&str], size: u32) -> Option<CursorImage> {
    let file = names.iter().map(|n| theme.dir.join("cursors").join(n)).find(|p| p.exists())?;
    read_xcursor(&std::fs::read(file).ok()?, size)
}

/// Reads an Xcursor file: a table of contents pointing at image chunks of several sizes.
fn read_xcursor(data: &[u8], size: u32) -> Option<CursorImage> {
    const IMAGE: u32 = 0xfffd_0002;
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if data.get(0..4)? != b"Xcur" {
        return None;
    }
    let header = u32_at(4)? as usize;
    let count = u32_at(12)? as usize;
    let (_, position) = (0..count.min(1024))
        .filter_map(|i| {
            let at = header + i * 12;
            (u32_at(at)? == IMAGE).then_some((u32_at(at + 4)?, u32_at(at + 8)? as usize))
        })
        .min_by_key(|(nominal, _)| nominal.abs_diff(size))?;
    let width = u32_at(position + 16)?;
    let height = u32_at(position + 20)?;
    let start = position + 36;
    let len = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return None;
    }
    // Pixels are little-endian ARGB u32s, premultiplied: in memory that's B, G, R, A.
    let pixels = data.get(start..start + len)?.to_vec();
    Some(CursorImage { width, height, pixels })
}

/// Applies a cursor theme beyond niri (which sets it for the apps it starts): GTK through
/// gsettings and its settings.ini files, Qt/KDE through kcminputrc. Best effort.
pub fn apply_cursor_to_toolkits(theme: Option<&str>, size: Option<u32>) {
    let quiet = |cmd: &mut Command| {
        let _ = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    };
    if let Some(theme) = theme {
        quiet(Command::new("gsettings").args(["set", "org.gnome.desktop.interface", "cursor-theme", theme]));
        quiet(Command::new("kwriteconfig6").args([
            "--file",
            "kcminputrc",
            "--group",
            "Mouse",
            "--key",
            "cursorTheme",
            theme,
        ]));
    }
    if let Some(size) = size {
        let size = size.to_string();
        quiet(Command::new("gsettings").args(["set", "org.gnome.desktop.interface", "cursor-size", &size]));
        quiet(Command::new("kwriteconfig6").args([
            "--file",
            "kcminputrc",
            "--group",
            "Mouse",
            "--key",
            "cursorSize",
            &size,
        ]));
    }
    for gtk in ["gtk-3.0", "gtk-4.0"] {
        let path = crate::config_dir().join(gtk).join("settings.ini");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let updated = update_ini(&text, theme, size);
        if updated != text {
            let _ = std::fs::write(&path, updated);
        }
    }
}

/// Replaces the cursor lines of a GTK settings.ini that has them.
fn update_ini(text: &str, theme: Option<&str>, size: Option<u32>) -> String {
    let mut out: String = text
        .lines()
        .map(|line| {
            let key = line.split('=').next().unwrap_or_default().trim();
            match (key, theme, size) {
                ("gtk-cursor-theme-name", Some(t), _) => format!("gtk-cursor-theme-name={t}"),
                ("gtk-cursor-theme-size", _, Some(s)) => format!("gtk-cursor-theme-size={s}"),
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effective_of(files: &[(&str, &str)]) -> InputConfig {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dmm-niri-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let mut config = InputConfig::default();
        read_into(&dir.join(files[0].0), &dir.join(FILE_NAME), &mut config, 0).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        config
    }

    const MAIN: &str = r#"input {
    keyboard {
        repeat-delay 250
        xkb { layout "us"; }
    }
    touchpad {
        tap
        natural-scroll
    }
}
cursor { hide-when-typing; }
include "local.kdl"
include optional=true "dm-manager.kdl"
"#;
    const LOCAL: &str = r#"input {
    keyboard {
        xkb {
            layout "us,ru"
            options "grp:alt_shift_toggle"
        }
    }
}
cursor {
    xcursor-theme "capitaine-cursors-light"
    xcursor-size 24
}
"#;

    #[test]
    fn follows_includes_and_merges() {
        let c = effective_of(&[("config.kdl", MAIN), ("local.kdl", LOCAL)]);
        assert!(c.included);
        assert_eq!(c.keyboard.number("repeat-delay"), Some(250.0));
        assert_eq!(c.layouts(), vec![("us".to_string(), String::new()), ("ru".to_string(), String::new())]);
        assert_eq!(c.xkb.string("options"), Some("grp:alt_shift_toggle"));
        assert!(c.touchpad.flag("tap"));
        assert!(c.cursor.flag("hide-when-typing"));
        assert_eq!(c.cursor.string("xcursor-theme"), Some("capitaine-cursors-light"));
    }

    #[test]
    fn touchpad_blocks_replace_each_other() {
        let own = "input {\n    touchpad {\n        dwt\n    }\n}\n";
        let c = effective_of(&[("config.kdl", MAIN), ("local.kdl", LOCAL), (FILE_NAME, own)]);
        assert!(!c.touchpad.flag("tap"));
        assert!(c.touchpad.flag("dwt"));
    }

    #[test]
    fn turning_off_a_touchpad_flag_copies_the_block() {
        let c = effective_of(&[("config.kdl", MAIN), ("local.kdl", LOCAL)]);
        let mut own = NiriFile { path: PathBuf::from("/nonexistent"), file: KdlText::parse_v1(HEADER.into()).unwrap() };
        own.set(&c, Area::Touchpad, "tap", Some(Value::Bool(false))).unwrap();
        assert!(own.text().ends_with("input {\n    touchpad {\n        natural-scroll\n    }\n}\n"), "{}", own.text());
        own.set(&c, Area::Touchpad, "accel-speed", Some(Value::Float(0.3))).unwrap();
        assert!(own.text().contains("        natural-scroll\n        accel-speed 0.3\n"), "{}", own.text());
    }

    #[test]
    fn xkb_is_written_whole() {
        let c = effective_of(&[("config.kdl", MAIN), ("local.kdl", LOCAL)]);
        let mut own = NiriFile { path: PathBuf::from("/nonexistent"), file: KdlText::parse_v1(HEADER.into()).unwrap() };
        own.set(&c, Area::Xkb, "layout", Some(Value::Str("us,de".into()))).unwrap();
        let text = own.text();
        assert!(text.contains("layout \"us,de\""), "{text}");
        assert!(text.contains("options \"grp:alt_shift_toggle\""), "{text}");
    }

    #[test]
    fn flags_outside_device_blocks_can_be_turned_off() {
        let c = effective_of(&[("config.kdl", MAIN)]);
        let mut own = NiriFile { path: PathBuf::from("/nonexistent"), file: KdlText::parse_v1(HEADER.into()).unwrap() };
        own.set(&c, Area::Cursor, "hide-when-typing", Some(Value::Bool(false))).unwrap();
        assert!(own.text().ends_with("cursor {\n    hide-when-typing false\n}\n"), "{}", own.text());
        own.set(&c, Area::Cursor, "hide-when-typing", Some(Value::Bool(true))).unwrap();
        assert!(own.text().ends_with("cursor {\n    hide-when-typing\n}\n"), "{}", own.text());
    }

    #[test]
    fn rebinds_a_spawn_key() {
        let main = "binds {\n    Mod+T hotkey-overlay-title=\"Terminal\" { spawn \"kitty\"; }\n    Mod+E { spawn \"dolphin\"; }\n}\n";
        let c = effective_of(&[("config.kdl", main)]);
        let bind = &c.binds["Mod+T"];
        assert_eq!(bind.spawn.as_deref(), Some("kitty"));
        let mut own = NiriFile { path: PathBuf::from("/nonexistent"), file: KdlText::parse_v1(HEADER.into()).unwrap() };
        own.rebind_spawn("Mod+T", bind, "alacritty").unwrap();
        assert!(
            own.text().ends_with("binds {\n    Mod+T hotkey-overlay-title=\"Terminal\" { spawn \"alacritty\"; }\n}\n"),
            "{}",
            own.text()
        );
        // Again: replaces, doesn't duplicate.
        own.rebind_spawn("Mod+T", bind, "foot").unwrap();
        assert_eq!(own.text().matches("Mod+T").count(), 1);
    }

    #[test]
    fn binds_from_files() {
        let main = "binds {\n    Mod+T hotkey-overlay-title=\"Terminal\" { spawn \"kitty\"; }\n    Mod+Q repeat=false { close-window; }\n}\ninclude optional=true \"dm-manager.kdl\"\n";
        let own = "binds {\n    Mod+T { spawn \"foot\"; }\n}\n";
        let c = effective_of(&[("config.kdl", main), (FILE_NAME, own)]);
        let t = &c.binds["Mod+T"];
        assert_eq!((t.action.as_str(), t.own), (r#"spawn "foot""#, true));
        assert_eq!(t.overrides.as_ref().map(|(_, a)| a.as_str()), Some(r#"spawn "kitty""#));
        let q = &c.binds["Mod+Q"];
        assert_eq!((q.action.as_str(), q.repeat, q.own, q.overrides.is_none()), ("close-window", false, false, true));
    }

    #[test]
    fn writes_and_removes_own_binds() {
        let mut own = NiriFile { path: PathBuf::from("/nonexistent"), file: KdlText::parse_v1(HEADER.into()).unwrap() };
        let spec = |key: &str, action: &str| crate::binds::BindSpec {
            key: key.into(),
            action: action.into(),
            title: None,
            repeat: true,
            allow_when_locked: false,
        };
        own.set_bind(None, &spec("Mod+T", r#"spawn "kitty""#)).unwrap();
        own.set_bind(None, &spec("Mod+E", r#"spawn "dolphin""#)).unwrap();
        assert!(
            own.text().ends_with("binds {\n    Mod+T { spawn \"kitty\"; }\n    Mod+E { spawn \"dolphin\"; }\n}\n"),
            "{}",
            own.text()
        );
        // Editing changes the key: the old one goes.
        own.set_bind(Some("Mod+T"), &spec("Mod+Return", r#"spawn "kitty""#)).unwrap();
        assert!(!own.text().contains("Mod+T "));
        assert_eq!(own.text().matches("Mod+Return").count(), 1);
        own.remove_bind("Mod+E").unwrap();
        assert!(!own.text().contains("Mod+E"));
    }

    #[test]
    fn comments_out_a_block() {
        let text = "// mine\noutput \"eDP-1\" {\n    variable-refresh-rate\n}\n\nbinds {}\n";
        let block = "output \"eDP-1\" {\n    variable-refresh-rate\n}";
        let out = comment_out(text, block).unwrap();
        assert_eq!(
            out,
            format!("// mine\n{MOVED}\n// output \"eDP-1\" {{\n//     variable-refresh-rate\n// }}\n\nbinds {{}}\n")
        );
        assert!(KdlText::parse_v1(out).is_ok());
    }

    #[test]
    fn writes_outputs_keeping_extras() {
        let mut own = NiriFile {
            path: PathBuf::from("/nonexistent"),
            file: KdlText::parse_v1("output \"eDP-1\" {\n    scale 2\n    focus-at-startup\n}\n".into()).unwrap(),
        };
        let settings = crate::outputs::Settings {
            on: true,
            mode: None,
            scale: 1.5,
            transform: crate::outputs::Transform::Normal,
            x: 0,
            y: 0,
            vrr: false,
        };
        own.set_output("eDP-1", &settings).unwrap();
        assert_eq!(
            own.text(),
            "output \"eDP-1\" {\n    scale 1.5\n    transform \"normal\"\n    position x=0 y=0\n    focus-at-startup\n}\n"
        );
    }

    #[test]
    fn first_output_block_applies() {
        let main = "output \"eDP-1\" {\n    scale 1.25\n}\ninclude \"dm-manager.kdl\"\n";
        let own = "output \"eDP-1\" {\n    scale 1.5\n}\noutput \"HDMI-A-1\" {\n    off\n}\n";
        let c = effective_of(&[("config.kdl", main), (FILE_NAME, own)]);
        assert_eq!(c.outputs.len(), 2);
        assert!(!c.outputs[0].own && c.outputs[0].raw.contains("1.25"));
        assert!(c.outputs[1].own && c.outputs[1].name == "HDMI-A-1");
    }

    #[test]
    fn xkb_rules() {
        let xml = r#"<xkbConfigRegistry><layoutList>
            <layout><configItem><name>ru</name><description>Russian</description></configItem>
              <variantList><variant><configItem><name>phonetic</name><description>Russian (phonetic)</description></configItem></variant></variantList>
            </layout>
            <layout><configItem><name>us</name><description>English (US)</description></configItem></layout>
          </layoutList><optionList>
            <group><configItem><name>grp</name><description>Switching</description></configItem>
              <option><configItem><name>grp:alt_shift_toggle</name><description>Alt+Shift</description></configItem></option>
            </group></optionList></xkbConfigRegistry>"#;
        let c = parse_xkb_rules(xml);
        assert_eq!(c.layouts[0].name, "us");
        assert_eq!(c.describe("ru", "phonetic"), "Russian (phonetic)");
        assert_eq!(c.describe("ru", ""), "Russian");
        assert_eq!(c.switch_options[0].name, "grp:alt_shift_toggle");
    }

    #[test]
    fn layout_strings() {
        let pairs = |p: &[(&str, &str)]| p.iter().map(|(l, v)| (l.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(join_layouts(&pairs(&[("us", ""), ("ru", "")])), ("us,ru".into(), None));
        assert_eq!(join_layouts(&pairs(&[("us", ""), ("ru", "phonetic")])), ("us,ru".into(), Some(",phonetic".into())));
        assert_eq!(switch_option("caps:escape, grp:alt_shift_toggle"), Some("grp:alt_shift_toggle"));
        assert_eq!(
            with_switch_option("caps:escape,grp:alt_shift_toggle", Some("grp:win_space_toggle")),
            Some("caps:escape,grp:win_space_toggle".into())
        );
        assert_eq!(with_switch_option("grp:alt_shift_toggle", None), None);
    }

    #[test]
    fn localectl_layouts() {
        let status = "   X11 Layout: us,ru\n    X11 Model: pc105\n  X11 Variant: ,phonetic\n";
        assert_eq!(parse_localectl(status), vec![("us".into(), "".into()), ("ru".into(), "phonetic".into())]);
        assert!(parse_localectl("    VC Keymap: us\n").is_empty());
    }

    #[test]
    fn ini_cursor_lines() {
        let ini = "[Settings]\ngtk-cursor-theme-name=old\ngtk-font-name=Inter 11\n";
        assert_eq!(
            update_ini(ini, Some("new"), Some(32)),
            "[Settings]\ngtk-cursor-theme-name=new\ngtk-font-name=Inter 11\n"
        );
    }

    #[test]
    fn xcursor() {
        // Header (16 bytes), one TOC entry, one 1x1 image chunk.
        let mut data = b"Xcur".to_vec();
        for n in [16u32, 0x1_0000, 1, 0xfffd_0002, 24, 28, 36, 0xfffd_0002, 24, 1, 1, 1, 0, 0, 0, 0xff10_2030] {
            data.extend(n.to_le_bytes());
        }
        let img = read_xcursor(&data, 32).unwrap();
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.pixels, vec![0x30, 0x20, 0x10, 0xff]);
    }
}
