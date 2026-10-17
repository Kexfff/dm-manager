//! Edits dm-manager.kdl the way the Keyboard & Input page does and saves it (validated by
//! niri). Point XDG_CONFIG_HOME at a copy of your config: it writes to $XDG_CONFIG_HOME/niri.

use dmm_sys::kdl_text::Value;
use dmm_sys::niri::{self, Area, NiriFile};

fn main() -> Result<(), String> {
    let effective = niri::effective();
    assert!(effective.included, "the config must include dm-manager.kdl");
    let mut file = NiriFile::load()?;

    let mut layouts = effective.layouts();
    layouts.push(("de".into(), "nodeadkeys".into()));
    let (layout, variant) = niri::join_layouts(&layouts);
    file.set(&effective, Area::Xkb, "layout", Some(Value::Str(layout)))?;
    file.set(&effective, Area::Xkb, "variant", variant.map(Value::Str))?;
    let options =
        niri::with_switch_option(effective.xkb.string("options").unwrap_or_default(), Some("grp:win_space_toggle"));
    file.set(&effective, Area::Xkb, "options", options.map(Value::Str))?;
    file.set(&effective, Area::Keyboard, "repeat-delay", Some(Value::Int(300)))?;
    file.set(&effective, Area::Touchpad, "tap", Some(Value::Bool(false)))?;
    file.set(&effective, Area::Touchpad, "accel-speed", Some(Value::Float(0.25)))?;
    file.set(&effective, Area::Cursor, "hide-when-typing", Some(Value::Bool(false)))?;
    file.set(&effective, Area::Cursor, "xcursor-size", Some(Value::Int(32)))?;
    file.save()?;
    println!("{}", file.text());

    let after = niri::effective();
    println!("layouts {:?}", after.layouts());
    println!("options {:?}", after.xkb.string("options"));
    println!(
        "tap {} natural-scroll {} accel {:?}",
        after.touchpad.flag("tap"),
        after.touchpad.flag("natural-scroll"),
        after.touchpad.number("accel-speed")
    );
    println!(
        "hide-when-typing {} size {:?}",
        after.cursor.flag("hide-when-typing"),
        after.cursor.number("xcursor-size")
    );
    Ok(())
}
