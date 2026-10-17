//! Prints the appearance settings in effect and what's installed (read-only).

use dmm_sys::appearance;

fn main() {
    println!("{:#?}", appearance::current());
    let schemes = appearance::color_schemes();
    println!("{} color schemes, e.g. {:?}", schemes.len(), schemes.iter().take(3).map(|s| &s.name).collect::<Vec<_>>());
    println!("styles {:?}", appearance::widget_styles().iter().map(|s| &s.id).collect::<Vec<_>>());
    println!("kvantum {:?}", appearance::kvantum_themes());
    println!("gtk {:?}", appearance::gtk_themes());
    println!("icons {:?}", appearance::icon_themes().iter().map(|t| &t.id).collect::<Vec<_>>());
}
