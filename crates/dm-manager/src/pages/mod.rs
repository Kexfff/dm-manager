pub mod appearance;
pub mod apps;
pub mod binds;
pub mod displays;
pub mod bluetooth;
pub mod input;
pub mod overview;
pub mod power;
pub mod shell;
pub mod wallpapers;
pub mod wifi;

use relm4::adw;
use relm4::gtk;
use relm4::gtk::prelude::*;

/// A page that isn't built yet: what it will do.
pub fn placeholder(icon: &str, title: &str, description: &str) -> adw::StatusPage {
    let soon = gtk::Label::builder().label("Coming soon").halign(gtk::Align::Center).css_classes(["soon"]).build();
    let page = adw::StatusPage::builder().icon_name(icon).title(title).description(description).child(&soon).build();
    page.add_css_class("compact");
    page
}
