//! dm-manager's own settings: `~/.config/dm-manager/settings.kdl`, one `key "value"` per line.

use std::path::PathBuf;

fn path() -> PathBuf {
    dmm_sys::config_dir().join("dm-manager").join("settings.kdl")
}

fn read() -> kdl::KdlDocument {
    std::fs::read_to_string(path()).ok().and_then(|text| text.parse().ok()).unwrap_or_default()
}

pub fn get(key: &str) -> Option<String> {
    read().get_arg(key).and_then(|v| v.as_string()).map(String::from)
}

/// Sets (or with `None` removes) a value, keeping the rest of the file.
pub fn set(key: &str, value: Option<&str>) {
    let mut doc = read();
    doc.nodes_mut().retain(|n| n.name().value() != key);
    if let Some(value) = value {
        let mut node = kdl::KdlNode::new(key);
        node.push(kdl::KdlEntry::new(value));
        doc.nodes_mut().push(node);
    }
    doc.autoformat();
    let path = path();
    let written =
        path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, doc.to_string()));
    if let Err(e) = written {
        log::warn!("can't save {}: {e}", path.display());
    }
}
