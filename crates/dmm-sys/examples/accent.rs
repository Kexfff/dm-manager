//! Sets the Qt/KDE accent, switches scheme, then goes back to the scheme's colors. Run it on
//! a copy: XDG_CONFIG_HOME=<copy> cargo run --example accent

use dmm_sys::appearance;

fn main() -> Result<(), String> {
    let show = |step: &str| {
        let c = appearance::current();
        println!("{step}: scheme {:?}, accent {:?}, palette {:?}", c.color_scheme, c.accent_color, c.palette_accent);
    };
    show("before");
    appearance::set_accent_color(Some((233, 84, 32)))?;
    show("accent set");
    appearance::set_color_scheme("BreezeDark")?;
    show("new scheme");
    appearance::set_accent_color(None)?;
    show("scheme's own");
    Ok(())
}
