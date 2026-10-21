//! The window: a sidebar of pages and the selected page beside it.

use relm4::adw::prelude::*;
use relm4::{Component, ComponentController, ComponentParts, ComponentSender, Controller, adw, gtk};

use crate::pages::{self, appearance, apps, bluetooth, input, overview, power, shell, wallpapers, wifi};
use crate::theme;

struct PageInfo {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
    section: usize,
    /// For pages not built yet: what they will do.
    soon: Option<&'static str>,
}

/// Sidebar sections; the first, with the overview, has no title.
const SECTIONS: [Option<&str>; 4] = [None, Some("Connections"), Some("System"), Some("Tools")];

const PAGES: &[PageInfo] = &[
    PageInfo { id: "overview", title: "Overview", icon: "dmm-computer-symbolic", section: 0, soon: None },
    PageInfo { id: "wifi", title: "Wi-Fi", icon: "dmm-network-wireless-symbolic", section: 1, soon: None },
    PageInfo { id: "bluetooth", title: "Bluetooth", icon: "dmm-bluetooth-active-symbolic", section: 1, soon: None },
    PageInfo { id: "shell", title: "Shell", icon: "dmm-shell-symbolic", section: 2, soon: None },
    PageInfo { id: "input", title: "Keyboard & Input", icon: "dmm-input-keyboard-symbolic", section: 2, soon: None },
    PageInfo { id: "apps", title: "Default Apps", icon: "dmm-view-app-grid-symbolic", section: 2, soon: None },
    PageInfo {
        id: "appearance",
        title: "Appearance",
        icon: "dmm-preferences-desktop-appearance-symbolic",
        section: 2,
        soon: None,
    },
    PageInfo { id: "power", title: "Power & Sleep", icon: "dmm-weather-clear-night-symbolic", section: 2, soon: None },
    PageInfo {
        id: "wallpapers",
        title: "Wallpapers",
        icon: "dmm-preferences-desktop-wallpaper-symbolic",
        section: 3,
        soon: None,
    },
    PageInfo {
        id: "assistant",
        title: "Assistant",
        icon: "dmm-assistant-symbolic",
        section: 3,
        soon: Some("An AI assistant that knows your desktop and can change settings for you."),
    },
    PageInfo {
        id: "monitor",
        title: "System Monitor",
        icon: "dmm-monitor-symbolic",
        section: 3,
        soon: Some("CPU, memory, disks, network and processes at a glance."),
    },
];

pub struct App {
    overview: Controller<overview::OverviewPage>,
    wifi: Controller<wifi::WifiPage>,
    bluetooth: Controller<bluetooth::BluetoothPage>,
    shell: Controller<shell::ShellPage>,
    input: Controller<input::InputPage>,
    apps: Controller<apps::AppsPage>,
    appearance: Controller<appearance::AppearancePage>,
    power: Controller<power::PowerPage>,
    wallpapers: Controller<wallpapers::WallpapersPage>,
    current: usize,
}

#[derive(Debug)]
pub enum Msg {
    Select(u32),
    Toast(String),
    Accent(usize),
}

pub struct Widgets {
    split: adw::NavigationSplitView,
    content_page: adw::NavigationPage,
    stack: gtk::Stack,
    toasts: adw::ToastOverlay,
}

impl Component for App {
    type CommandOutput = ();
    type Input = Msg;
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .title("DM Manager")
            .icon_name(crate::APP_ID)
            .default_width(1100)
            .default_height(720)
            .width_request(360)
            .height_request(420)
            .build()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        theme::init();

        let overview = overview::OverviewPage::builder()
            .launch(())
            .forward(sender.input_sender(), |overview::Output::Toast(text)| Msg::Toast(text));
        let wifi = wifi::WifiPage::builder()
            .launch(())
            .forward(sender.input_sender(), |wifi::Output::Toast(text)| Msg::Toast(text));
        let bluetooth = bluetooth::BluetoothPage::builder()
            .launch(())
            .forward(sender.input_sender(), |bluetooth::Output::Toast(text)| Msg::Toast(text));
        let shell = shell::ShellPage::builder()
            .launch(())
            .forward(sender.input_sender(), |shell::Output::Toast(text)| Msg::Toast(text));
        let input = input::InputPage::builder()
            .launch(())
            .forward(sender.input_sender(), |input::Output::Toast(text)| Msg::Toast(text));
        let apps = apps::AppsPage::builder()
            .launch(())
            .forward(sender.input_sender(), |apps::Output::Toast(text)| Msg::Toast(text));
        let appearance = appearance::AppearancePage::builder()
            .launch(())
            .forward(sender.input_sender(), |appearance::Output::Toast(text)| Msg::Toast(text));
        let power = power::PowerPage::builder()
            .launch(())
            .forward(sender.input_sender(), |power::Output::Toast(text)| Msg::Toast(text));
        let wallpapers = wallpapers::WallpapersPage::builder()
            .launch(())
            .forward(sender.input_sender(), |wallpapers::Output::Toast(text)| Msg::Toast(text));

        let stack =
            gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).transition_duration(160).build();
        stack.add_named(overview.widget(), Some("overview"));
        stack.add_named(wifi.widget(), Some("wifi"));
        stack.add_named(bluetooth.widget(), Some("bluetooth"));
        stack.add_named(shell.widget(), Some("shell"));
        stack.add_named(input.widget(), Some("input"));
        stack.add_named(apps.widget(), Some("apps"));
        stack.add_named(appearance.widget(), Some("appearance"));
        stack.add_named(power.widget(), Some("power"));
        stack.add_named(wallpapers.widget(), Some("wallpapers"));
        for page in PAGES {
            if let Some(description) = page.soon {
                stack.add_named(&pages::placeholder(page.icon, page.title, description), Some(page.id));
            }
        }

        let sidebar = adw::Sidebar::new();
        for (index, name) in SECTIONS.iter().enumerate() {
            let section = adw::SidebarSection::new();
            section.set_title(*name);
            for page in PAGES.iter().filter(|p| p.section == index) {
                let item = adw::SidebarItem::builder().title(page.title).icon_name(page.icon).build();
                if page.soon.is_some() {
                    let soon =
                        gtk::Label::builder().label("Soon").valign(gtk::Align::Center).css_classes(["soon"]).build();
                    item.set_suffix(Some(&soon));
                }
                section.append(item);
            }
            sidebar.append(section);
        }
        let s = sender.clone();
        sidebar.connect_selected_notify(move |sidebar| s.input(Msg::Select(sidebar.selected())));

        let brand = gtk::Label::builder().label("DM Manager").css_classes(["brand"]).build();
        let sidebar_header = adw::HeaderBar::builder().title_widget(&brand).build();
        sidebar_header.pack_end(&accent_button(&sender));
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.set_content(Some(&sidebar));
        let sidebar_page = adw::NavigationPage::builder().title("DM Manager").child(&sidebar_view).build();

        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
        content_view.set_content(Some(&stack));
        let content_page = adw::NavigationPage::builder().title(PAGES[0].title).child(&content_view).build();

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .min_sidebar_width(250.0)
            .max_sidebar_width(300.0)
            .sidebar_width_fraction(0.28)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));
        root.set_content(Some(&toasts));

        let narrow =
            adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 560sp").expect("valid condition"));
        narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
        root.add_breakpoint(narrow);

        if let Some(index) = std::env::var("DMM_PAGE").ok().and_then(|id| PAGES.iter().position(|p| p.id == id)) {
            sidebar.set_selected(index as u32);
        }
        if let Some(path) = std::env::var_os("DMM_SCREENSHOT") {
            stack.set_transition_type(gtk::StackTransitionType::None);
            screenshot_later(&root, path.into());
        }

        let model = Self { overview, wifi, bluetooth, shell, input, apps, appearance, power, wallpapers, current: 0 };
        let widgets = Widgets { split, content_page, stack, toasts };
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, msg: Msg, _: ComponentSender<Self>, _: &Self::Root) {
        match msg {
            Msg::Select(index) => {
                let Some(page) = PAGES.get(index as usize) else {
                    return;
                };
                w.stack.set_visible_child_name(page.id);
                w.content_page.set_title(page.title);
                w.split.set_show_content(true);
                if self.current != index as usize {
                    self.set_visible(self.current, false);
                    self.current = index as usize;
                    self.set_visible(self.current, true);
                }
            }
            Msg::Toast(text) => {
                let toast = adw::Toast::builder().title(text).use_markup(false).timeout(4).build();
                w.toasts.add_toast(toast);
            }
            Msg::Accent(index) => {
                let accent = &theme::ACCENTS[index];
                theme::apply_accent(accent);
                theme::save_accent(accent);
            }
        }
    }
}

impl App {
    /// Tells a page it was shown or hidden (so it can scan, or stop searching).
    fn set_visible(&self, index: usize, visible: bool) {
        match PAGES[index].id {
            "overview" => self.overview.emit(overview::Input::Visible(visible)),
            "wifi" => self.wifi.emit(wifi::Input::Visible(visible)),
            "bluetooth" => self.bluetooth.emit(bluetooth::Input::Visible(visible)),
            "shell" => self.shell.emit(shell::Input::Visible(visible)),
            "input" => self.input.emit(input::Input::Visible(visible)),
            "apps" => self.apps.emit(apps::Input::Visible(visible)),
            "appearance" => self.appearance.emit(appearance::Input::Visible(visible)),
            "power" => self.power.emit(power::Input::Visible(visible)),
            "wallpapers" => self.wallpapers.emit(wallpapers::Input::Visible(visible)),
            _ => {}
        }
    }
}

/// Development aid: renders the window to a PNG once it has settled, then quits.
/// `DMM_SCREENSHOT=/tmp/shot.png DMM_PAGE=bluetooth dm-manager`
fn screenshot_later(window: &adw::ApplicationWindow, path: std::path::PathBuf) {
    let window = window.clone();
    let mut attempts = 0;
    let delay = std::env::var("DMM_SCREENSHOT_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(2500);
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(delay), move || {
        // The window's top child (libadwaita's dialog host): content and any open dialog.
        let content = gtk::prelude::GtkWindowExt::child(&window).expect("window content");
        let paintable = gtk::WidgetPaintable::new(Some(&content));
        let (width, height) = (content.width() as f32, content.height() as f32);
        let drawn = gtk::Snapshot::new();
        paintable.snapshot(&drawn, width as f64, height as f64);
        let Some(drawn) = drawn.to_node() else {
            // Not drawn yet; try again.
            attempts += 1;
            return if attempts < 10 { gtk::glib::ControlFlow::Continue } else { gtk::glib::ControlFlow::Break };
        };
        // The window draws its background itself, not its content.
        let snapshot = gtk::Snapshot::new();
        let background = gtk::gdk::RGBA::parse("#0c0c0e").expect("valid color");
        snapshot.append_color(&background, &gtk::graphene::Rect::new(0.0, 0.0, width, height));
        snapshot.append_node(&drawn);
        let node = snapshot.to_node().expect("a background at least");
        let renderer = gtk::gsk::CairoRenderer::new();
        match renderer.realize_for_display(&WidgetExt::display(&window)) {
            Ok(()) => {
                if let Err(e) = renderer.render_texture(&node, None).save_to_png(&path) {
                    log::error!("can't save {}: {e}", path.display());
                }
                renderer.unrealize();
            }
            Err(e) => log::error!("can't render the window: {e}"),
        }
        if let Some(app) = window.application() {
            app.quit();
        }
        gtk::glib::ControlFlow::Break
    });
}

/// The colored dot in the sidebar header that opens the accent picker.
fn accent_button(sender: &ComponentSender<App>) -> gtk::MenuButton {
    let grid = gtk::Grid::builder().column_spacing(10).row_spacing(10).build();
    let selected = theme::accent_index(theme::saved_accent());
    let mut first: Option<gtk::ToggleButton> = None;
    for (index, accent) in theme::ACCENTS.iter().enumerate() {
        let check = gtk::Image::from_icon_name("dmm-object-select-symbolic");
        let swatch = gtk::ToggleButton::builder()
            .child(&check)
            .tooltip_text(accent.name)
            .css_classes(["swatch", &format!("swatch-{}", accent.id)])
            .active(index == selected)
            .build();
        swatch.set_group(first.as_ref());
        let s = sender.clone();
        swatch.connect_toggled(move |b| {
            if b.is_active() {
                s.input(Msg::Accent(index));
            }
        });
        grid.attach(&swatch, (index % 5) as i32, (index / 5) as i32, 1, 1);
        first.get_or_insert(swatch);
    }

    let title = gtk::Label::builder().label("Accent Color").xalign(0.0).css_classes(["heading"]).build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    content.append(&title);
    content.append(&grid);
    let popover = gtk::Popover::builder().child(&content).build();

    let dot =
        gtk::Box::builder().css_classes(["accent-dot"]).halign(gtk::Align::Center).valign(gtk::Align::Center).build();
    gtk::MenuButton::builder()
        .child(&dot)
        .popover(&popover)
        .tooltip_text("Accent color")
        .css_classes(["flat", "circular"])
        .build()
}
