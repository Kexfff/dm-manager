//! Edits a copy of a dmsh config the way the Shell page does, saves it (validated by dmsh)
//! and renders a preview: `dmsh_edit <config.kdl> <preview.png>`.

use dmm_sys::dmsh::{self, DmshConfig, Preview, SETTINGS};
use dmm_sys::kdl_text::Value;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = std::path::PathBuf::from(args.next().ok_or("config path")?);
    let preview = std::path::PathBuf::from(args.next().ok_or("preview path")?);
    let find = |section: &str, key: &str| SETTINGS.iter().find(|s| s.section == section && s.key == key).unwrap();

    let mut c = DmshConfig::load(&path)?;
    c.set(find("theme", "pill-color"), &Value::Str("#1c1c1e".into()))?;
    c.set(find("theme", "corner-radius"), &Value::Float(18.0))?;
    c.set(find("timing", "osd"), &Value::Str(dmsh::format_duration(2.5)))?;
    c.set(find("sources", "tray"), &Value::Bool(false))?;
    c.set_animations(false)?;
    c.save()?;
    dmsh::render_preview(c.text(), Preview::Right, &preview)?;

    // A bad value must be refused, leaving the file as it was.
    let before = std::fs::read_to_string(&path).unwrap();
    c.set(find("theme", "pill-height"), &Value::Float(200.0))?;
    println!("bad value: {:?}", c.save());
    assert_eq!(before, std::fs::read_to_string(&path).unwrap());
    Ok(())
}
