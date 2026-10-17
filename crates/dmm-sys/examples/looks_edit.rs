//! Applies appearance, terminal and idle changes the way the pages do. Run it against copies:
//! XDG_CONFIG_HOME=<copy> GSETTINGS_BACKEND=memory cargo run --example looks_edit

use dmm_sys::idle::{IdleConfig, Timer};
use dmm_sys::{appearance, apps};

fn main() -> Result<(), String> {
    appearance::set_color_scheme("BreezeDark")?;
    appearance::set_widget_style("Breeze")?;
    appearance::set_kvantum_theme("KvArcDark")?;
    appearance::set_icon_theme("Papirus-Dark")?;
    appearance::set_gtk_theme("Breeze-Dark")?;
    appearance::set_gtk_color_scheme("prefer-dark")?;
    appearance::set_font(false, "Inter", 10.5)?;
    appearance::set_font(true, "JetBrains Mono", 11.0)?;
    appearance::set_gtk_css(false)?;
    apps::set_terminal("Alacritty.desktop", "alacritty")?;
    println!("{:#?}", appearance::current());
    println!("terminal {:?}", apps::current_terminal());

    let mut idle = IdleConfig::load(&IdleConfig::default_path());
    idle.set_timeout(Timer::Lock, Some(900));
    idle.set_timeout(Timer::Suspend, Some(1800));
    idle.set_lock_before_sleep(false);
    idle.save()
}
