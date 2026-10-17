//! Prints the niri input and cursor settings in effect, and what the catalogs find (read-only).

use dmm_sys::niri;

fn main() {
    let config = niri::effective();
    println!("{config:#?}");
    println!("layouts: {:?}", config.layouts());
    let catalog = niri::xkb_catalog();
    println!(
        "{} layouts, {} switch options",
        catalog.layouts.len(),
        catalog.switch_options.len()
    );
    for (l, v) in config.layouts() {
        println!("  {l}/{v}: {}", catalog.describe(&l, &v));
    }
    println!("touchpad: {}", niri::has_touchpad());
    for theme in niri::cursor_themes() {
        let image = niri::cursor_image(&theme, &["default", "left_ptr"], 32);
        println!(
            "cursor theme {} ({}): {:?}",
            theme.id,
            theme.name,
            image.map(|i| (i.width, i.height))
        );
    }
}
