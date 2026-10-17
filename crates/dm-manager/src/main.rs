mod app;
mod pages;
mod theme;
mod widgets;

use relm4::gtk::gio;

const APP_ID: &str = "io.github.kexfff.DmManager";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    gio::resources_register_include!("resources.gresource").expect("bundled resources");
    relm4::RelmApp::new(APP_ID).run::<app::App>(());
}
