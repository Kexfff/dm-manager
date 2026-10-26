//! Sets up niri's config for dm-manager the way the Keyboard & Input page's button does:
//! creates ~/.config/niri/config.kdl from niri's default if needed and adds the include.

use dmm_sys::niri;

fn main() -> Result<(), String> {
    let before = niri::effective();
    println!("before: missing {}, included {}, can add {}", before.missing, before.included, niri::can_add_include());
    if !before.included {
        niri::add_include()?;
    }
    let after = niri::effective();
    println!("after: missing {}, included {}, error {:?}", after.missing, after.included, after.error);
    println!("layouts {:?}, xkb catalog {} layouts", after.layouts(), niri::xkb_catalog().layouts.len());
    Ok(())
}
