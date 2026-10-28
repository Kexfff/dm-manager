//! Editing KDL files in place.
//!
//! The file is parsed to read values, but changes are patched into the original text at the
//! parsed spans. Comments, blank lines, indentation and the file's KDL version (v1 like
//! niri's, or v2) all survive; only the touched values change.

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V1,
    V2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn from_kdl(value: &KdlValue) -> Option<Self> {
        Some(match value {
            KdlValue::String(s) => Self::Str(s.clone()),
            KdlValue::Integer(n) => Self::Int(i64::try_from(*n).ok()?),
            KdlValue::Float(n) => Self::Float(*n),
            KdlValue::Bool(b) => Self::Bool(*b),
            KdlValue::Null => return None,
        })
    }

    /// How the value is written in a file of this version.
    pub fn repr(&self, version: Version) -> String {
        match self {
            Self::Str(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            Self::Int(n) => n.to_string(),
            // `{:?}` keeps a fractional part (`1.0`), so it stays a float.
            Self::Float(n) => format!("{n:?}"),
            Self::Bool(b) => match version {
                Version::V1 => b.to_string(),
                Version::V2 => format!("#{b}"),
            },
        }
    }
}

pub struct KdlText {
    text: String,
    version: Version,
    doc: KdlDocument,
}

impl KdlText {
    /// Parses KDL v2, falling back to v1.
    pub fn parse(text: String) -> Result<Self, String> {
        let (doc, version) = match KdlDocument::parse_v2(&text) {
            Ok(doc) => (doc, Version::V2),
            Err(v2) => match KdlDocument::parse_v1(&text) {
                Ok(doc) => (doc, Version::V1),
                Err(_) => return Err(describe(&text, &v2)),
            },
        };
        Ok(Self { text, version, doc })
    }

    /// Parses as KDL v1 only (niri's syntax), so new values are written the v1 way.
    pub fn parse_v1(text: String) -> Result<Self, String> {
        let doc = KdlDocument::parse_v1(&text).map_err(|e| describe(&text, &e))?;
        Ok(Self { text, version: Version::V1, doc })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn document(&self) -> &KdlDocument {
        &self.doc
    }

    /// The node at `path` (by name, through children). The last match wins, as in configs
    /// where a later setting overrides an earlier one.
    pub fn node(&self, path: &[&str]) -> Option<&KdlNode> {
        let (last, parents) = path.split_last()?;
        let mut doc = &self.doc;
        for name in parents {
            doc = find(doc, name)?.children()?;
        }
        find(doc, last)
    }

    /// The node's first argument, or its `prop=` property.
    pub fn get(&self, path: &[&str], prop: Option<&str>) -> Option<Value> {
        let node = self.node(path)?;
        entry(node, prop).and_then(|e| Value::from_kdl(e.value()))
    }

    /// All of the node's arguments.
    pub fn args(&self, path: &[&str]) -> Vec<Value> {
        self.node(path)
            .map(|n| {
                n.entries().iter().filter(|e| e.name().is_none()).filter_map(|e| Value::from_kdl(e.value())).collect()
            })
            .unwrap_or_default()
    }

    /// Sets the node's first argument (or `prop=`), creating the node and its parents as needed.
    pub fn set(&mut self, path: &[&str], prop: Option<&str>, value: &Value) -> Result<(), String> {
        let repr = value.repr(self.version);
        let written = match prop {
            Some(p) => format!("{p}={repr}"),
            None => repr,
        };
        let edit = match self.node(path) {
            Some(node) => match entry(node, prop) {
                Some(e) => Edit::replace(span(e.span()), written),
                None => Edit::insert(entries_end(node), format!(" {written}")),
            },
            None => return self.create(path, &[written]),
        };
        self.apply(edit)
    }

    /// Replaces all of the node's arguments, creating it as needed. No arguments removes it.
    pub fn set_args(&mut self, path: &[&str], values: &[Value]) -> Result<(), String> {
        if values.is_empty() {
            return self.remove(path);
        }
        let reprs: Vec<String> = values.iter().map(|v| v.repr(self.version)).collect();
        let Some(node) = self.node(path) else {
            return self.create(path, &reprs);
        };
        let args: Vec<&KdlEntry> = node.entries().iter().filter(|e| e.name().is_none()).collect();
        let edit = match (args.first(), args.last()) {
            (Some(first), Some(last)) => {
                let (start, _) = span(first.span());
                let (_, end) = span(last.span());
                Edit::replace((start, end), reprs.join(" "))
            }
            _ => Edit::insert(entries_end(node), format!(" {}", reprs.join(" "))),
        };
        self.apply(edit)
    }

    /// Makes sure a bare node (a flag like `off`) exists.
    pub fn set_flag(&mut self, path: &[&str]) -> Result<(), String> {
        if self.node(path).is_some() {
            return Ok(());
        }
        self.create(path, &[])
    }

    /// Inserts a node given as KDL text (e.g. copied from another file) under `parent`,
    /// re-indented to fit. An empty `parent` means the top level.
    pub fn insert_node_text(&mut self, parent: &[&str], node_text: &str) -> Result<(), String> {
        self.insert_lines(parent, dedent(node_text))
    }

    /// Removes the node (and its line, if it's alone on it).
    pub fn remove(&mut self, path: &[&str]) -> Result<(), String> {
        let Some(node) = self.node(path) else {
            return Ok(());
        };
        let at = span(node.span());
        self.remove_at(at)
    }

    /// The top-level node `name` whose first argument is `arg` (like `output "eDP-1"`).
    pub fn node_with_arg(&self, name: &str, arg: &str) -> Option<&KdlNode> {
        self.doc.nodes().iter().find(|n| {
            n.name().value() == name
                && n.entries().iter().find(|e| e.name().is_none()).and_then(|e| e.value().as_string()) == Some(arg)
        })
    }

    /// Removes the top-level `name "arg" { … }` node, if there is one.
    pub fn remove_with_arg(&mut self, name: &str, arg: &str) -> Result<(), String> {
        let Some(node) = self.node_with_arg(name, arg) else { return Ok(()) };
        let at = span(node.span());
        self.remove_at(at)
    }

    /// The text of a node, as written.
    pub fn node_text(&self, node: &KdlNode) -> &str {
        let (start, end) = span(node.span());
        &self.text[start..end]
    }

    /// Removes the text at a node's span, with its line when the node is alone on it.
    fn remove_at(&mut self, (mut start, mut end): (usize, usize)) -> Result<(), String> {
        let bytes = self.text.as_bytes();
        let line_start = self.text[..start].rfind('\n').map_or(0, |i| i + 1);
        let rest = &self.text[end..];
        let line_end = rest.find('\n').map_or(self.text.len(), |i| end + i + 1);
        let before_blank = self.text[line_start..start].trim().is_empty();
        let after_blank = self.text[end..line_end].trim_matches(|c: char| c.is_whitespace() || c == ';').is_empty();
        if before_blank && after_blank {
            start = line_start;
            end = line_end;
        } else if end < bytes.len() && bytes[end] == b';' {
            end += 1;
        }
        self.apply(Edit::replace((start, end), String::new()))
    }

    /// Creates the node at `path` with these entries, under the nearest existing parent.
    fn create(&mut self, path: &[&str], entries: &[String]) -> Result<(), String> {
        let (name, parent) = path.split_last().expect("a path");
        let mut leaf = name.to_string();
        for e in entries {
            leaf.push(' ');
            leaf.push_str(e);
        }
        self.insert_lines(parent, vec![leaf])
    }

    /// Inserts lines (unindented KDL) as the last children of `parent`, creating whichever
    /// parents are missing.
    fn insert_lines(&mut self, parent: &[&str], mut lines: Vec<String>) -> Result<(), String> {
        let mut existing = parent.len();
        while existing > 0 && self.node(&parent[..existing]).is_none() {
            existing -= 1;
        }
        for name in parent[existing..].iter().rev() {
            let mut wrapped = vec![format!("{name} {{")];
            wrapped.extend(lines.iter().map(|l| format!("    {l}")));
            wrapped.push("}".into());
            lines = wrapped;
        }

        let edit = if existing == 0 {
            // Top level: append at the end of the file.
            let mut block = String::new();
            if !self.text.is_empty() && !self.text.ends_with('\n') {
                block.push('\n');
            }
            if !self.text.trim().is_empty() {
                block.push('\n');
            }
            for l in &lines {
                block.push_str(l);
                block.push('\n');
            }
            Edit::insert(self.text.len(), block)
        } else {
            let at = self.node(&parent[..existing]).expect("exists");
            self.child_insertion(at, &lines)
        };
        self.apply(edit)
    }

    /// Inserts lines as the last children of `parent`.
    fn child_insertion(&self, parent: &KdlNode, lines: &[String]) -> Edit {
        let (start, end) = span(parent.span());
        let parent_indent = indentation(&self.text, start);
        let indent = parent
            .children()
            .and_then(|c| c.nodes().last())
            .map(|last| indentation(&self.text, span(last.span()).0))
            .unwrap_or_else(|| format!("{parent_indent}    "));
        let body: String = lines.iter().map(|l| format!("{indent}{l}\n")).collect();

        if parent.children().is_none() {
            return Edit::insert(end, format!(" {{\n{body}{parent_indent}}}"));
        }
        // The children block ends with `}` at the end of the node's span.
        let close = self.text[..end].rfind('}').unwrap_or(end);
        let line_start = self.text[..close].rfind('\n').map_or(0, |i| i + 1);
        if self.text[line_start..close].trim().is_empty() {
            Edit::insert(line_start, body)
        } else {
            // `}` shares a line with other content (`section { a 1; }`).
            Edit::insert(close, format!("\n{body}{parent_indent}"))
        }
    }

    fn apply(&mut self, edit: Edit) -> Result<(), String> {
        let mut text = self.text.clone();
        text.replace_range(edit.start..edit.end, &edit.text);
        let doc = match self.version {
            Version::V2 => KdlDocument::parse_v2(&text),
            Version::V1 => KdlDocument::parse_v1(&text),
        }
        .map_err(|e| format!("edit produced invalid KDL: {}", describe(&text, &e)))?;
        self.text = text;
        self.doc = doc;
        Ok(())
    }
}

struct Edit {
    start: usize,
    end: usize,
    text: String,
}

impl Edit {
    fn replace((start, end): (usize, usize), text: String) -> Self {
        Self { start, end, text }
    }

    fn insert(at: usize, text: String) -> Self {
        Self { start: at, end: at, text }
    }
}

fn find<'a>(doc: &'a KdlDocument, name: &str) -> Option<&'a KdlNode> {
    doc.nodes().iter().rev().find(|n| n.name().value() == name)
}

fn entry<'a>(node: &'a KdlNode, prop: Option<&str>) -> Option<&'a KdlEntry> {
    node.entries().iter().find(|e| match prop {
        Some(p) => e.name().is_some_and(|n| n.value() == p),
        None => e.name().is_none(),
    })
}

fn span(s: miette::SourceSpan) -> (usize, usize) {
    (s.offset(), s.offset() + s.len())
}

/// Where new entries go: after the last entry, or after the node's name.
fn entries_end(node: &KdlNode) -> usize {
    match node.entries().last() {
        Some(e) => span(e.span()).1,
        None => span(node.name().span()).1,
    }
}

/// The lines of a block with their common indentation removed (the first line, which starts
/// at the node's name, has none).
fn dedent(block: &str) -> Vec<String> {
    let mut lines = block.lines();
    let first = lines.next().unwrap_or_default().trim_start().to_string();
    let rest: Vec<&str> = lines.collect();
    // The closing brace sits at the node's own indentation.
    let base = rest.last().map(|l| l.len() - l.trim_start().len()).unwrap_or(0);
    let mut out = vec![first];
    out.extend(rest.iter().map(|l| {
        let cut = (l.len() - l.trim_start().len()).min(base);
        l[cut..].to_string()
    }));
    out
}

/// The whitespace before the token at `at` on its line.
fn indentation(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..at].chars().take_while(|c| c.is_whitespace()).collect()
}

/// A parse error with its line number.
pub fn describe(text: &str, e: &kdl::KdlError) -> String {
    let Some(d) = e.diagnostics.first() else {
        return e.to_string();
    };
    let line = text[..d.span.offset().min(text.len())].matches('\n').count() + 1;
    let message = d.message.clone().unwrap_or_else(|| "invalid KDL".into());
    format!("line {line}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "// dmsh config\ntheme {\n    // The fill.\n    pill-color \"#000000\"\n    accent-colors low=\"#8e8e93\" normal=\"#f5f5f7\"\n}\n\nsources {\n    tray true\n}\n";

    fn edited(text: &str, f: impl FnOnce(&mut KdlText)) -> String {
        let mut k = KdlText::parse(text.to_string()).unwrap();
        f(&mut k);
        k.text().to_string()
    }

    #[test]
    fn detects_version() {
        assert_eq!(KdlText::parse(V1.into()).unwrap().version, Version::V1);
        assert_eq!(KdlText::parse("a #true\n".into()).unwrap().version, Version::V2);
    }

    #[test]
    fn reads_values() {
        let k = KdlText::parse(V1.into()).unwrap();
        assert_eq!(k.get(&["theme", "pill-color"], None), Some(Value::Str("#000000".into())));
        assert_eq!(k.get(&["theme", "accent-colors"], Some("normal")), Some(Value::Str("#f5f5f7".into())));
        assert_eq!(k.get(&["sources", "tray"], None), Some(Value::Bool(true)));
        assert_eq!(k.get(&["sources", "media"], None), None);
    }

    #[test]
    fn replaces_a_value_keeping_everything_else() {
        let out = edited(V1, |k| k.set(&["theme", "pill-color"], None, &Value::Str("#101010".into())).unwrap());
        assert_eq!(out, V1.replace("\"#000000\"", "\"#101010\""));
    }

    #[test]
    fn replaces_a_property() {
        let out =
            edited(V1, |k| k.set(&["theme", "accent-colors"], Some("low"), &Value::Str("#111111".into())).unwrap());
        assert_eq!(out, V1.replace("low=\"#8e8e93\"", "low=\"#111111\""));
    }

    #[test]
    fn adds_a_missing_property() {
        let out =
            edited(V1, |k| k.set(&["theme", "accent-colors"], Some("high"), &Value::Str("#ff9f0a".into())).unwrap());
        assert_eq!(out, V1.replace("normal=\"#f5f5f7\"", "normal=\"#f5f5f7\" high=\"#ff9f0a\""));
    }

    #[test]
    fn booleans_follow_the_file_version() {
        let out = edited(V1, |k| k.set(&["sources", "tray"], None, &Value::Bool(false)).unwrap());
        assert!(out.contains("    tray false\n"));
        let out = edited("sources {\n    tray #true\n}\n", |k| {
            k.set(&["sources", "tray"], None, &Value::Bool(false)).unwrap()
        });
        assert_eq!(out, "sources {\n    tray #false\n}\n");
    }

    #[test]
    fn adds_a_child_at_the_section_indentation() {
        let out = edited(V1, |k| k.set(&["sources", "media"], None, &Value::Bool(false)).unwrap());
        assert!(out.ends_with("sources {\n    tray true\n    media false\n}\n"), "{out}");
    }

    #[test]
    fn adds_a_missing_section() {
        let out = edited(V1, |k| k.set(&["clock", "format"], None, &Value::Str("%I:%M %p".into())).unwrap());
        assert!(out.starts_with(V1));
        assert!(out.ends_with("\nclock {\n    format \"%I:%M %p\"\n}\n"), "{out}");
    }

    #[test]
    fn creates_into_an_empty_file() {
        let out = edited("", |k| k.set(&["timing", "osd"], None, &Value::Str("2s".into())).unwrap());
        assert_eq!(out, "timing {\n    osd \"2s\"\n}\n");
    }

    #[test]
    fn creates_nested_sections() {
        let out = edited("input {\n}\n", |k| {
            k.set(&["input", "keyboard", "xkb", "layout"], None, &Value::Str("us".into())).unwrap()
        });
        assert_eq!(out, "input {\n    keyboard {\n        xkb {\n            layout \"us\"\n        }\n    }\n}\n");
    }

    #[test]
    fn removes_a_node_and_its_line() {
        let out = edited(V1, |k| k.remove(&["theme", "pill-color"]).unwrap());
        assert_eq!(out, V1.replace("    pill-color \"#000000\"\n", ""));
    }

    #[test]
    fn flags_and_args() {
        let out = edited("animation {\n    response 0.42\n}\n", |k| k.set_flag(&["animation", "off"]).unwrap());
        assert_eq!(out, "animation {\n    response 0.42\n    off\n}\n");
        let out = edited("outputs {\n    only \"eDP-1\"\n}\n", |k| {
            k.set_args(&["outputs", "only"], &[Value::Str("eDP-1".into()), Value::Str("HDMI-A-1".into())]).unwrap()
        });
        assert_eq!(out, "outputs {\n    only \"eDP-1\" \"HDMI-A-1\"\n}\n");
    }

    #[test]
    fn escapes_strings_and_writes_floats() {
        assert_eq!(Value::Str("a \"b\" \\".into()).repr(Version::V1), r#""a \"b\" \\""#);
        assert_eq!(Value::Float(1.0).repr(Version::V1), "1.0");
        assert_eq!(Value::Float(0.42).repr(Version::V2), "0.42");
    }

    #[test]
    fn copies_a_node_from_another_file() {
        let source = "input {\n        touchpad {\n            tap\n            accel-speed 0.2\n        }\n}";
        let k = KdlText::parse_v1(source.into()).unwrap();
        let touchpad = k.node(&["input", "touchpad"]).unwrap();
        let (start, end) = span(touchpad.span());
        let out = edited("// mine\n", |m| m.insert_node_text(&["input"], &source[start..end]).unwrap());
        assert_eq!(out, "// mine\n\ninput {\n    touchpad {\n        tap\n        accel-speed 0.2\n    }\n}\n");
    }

    #[test]
    fn single_line_children() {
        let out = edited("a { b 1; }\n", |k| k.set(&["a", "c"], None, &Value::Int(2)).unwrap());
        assert_eq!(KdlText::parse(out).unwrap().get(&["a", "c"], None), Some(Value::Int(2)));
    }
}
