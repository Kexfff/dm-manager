//! Overview: this computer at a glance, the page the app opens on.

use std::path::Path;

use dmm_sys::sysinfo::{self, SystemInfo};
use relm4::adw::prelude::*;
use relm4::gtk::gdk;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::tile;

pub struct OverviewPage {
    info: Option<SystemInfo>,
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Copy,
}

#[derive(Debug)]
pub enum Cmd {
    Read(Box<SystemInfo>),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    logo: gtk::Image,
    host: gtk::Label,
    device: gtk::Label,
    hardware: gtk::FlowBox,
    software: gtk::ListBox,
}

impl Component for OverviewPage {
    type CommandOutput = Cmd;
    type Input = Input;
    type Output = Output;
    type Init = ();
    type Root = adw::PreferencesPage;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        adw::PreferencesPage::new()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let logo = gtk::Image::builder().pixel_size(96).css_classes(["overview-logo"]).build();
        let host = gtk::Label::builder().css_classes(["overview-host"]).selectable(true).build();
        let device =
            gtk::Label::builder().css_classes(["dim-label"]).wrap(true).justify(gtk::Justification::Center).build();
        let copy = gtk::Button::builder()
            .child(&adw::ButtonContent::builder().icon_name("dmm-edit-copy-symbolic").label("Copy Info").build())
            .tooltip_text("Copy a summary of this system, e.g. for a bug report")
            .halign(gtk::Align::Center)
            .css_classes(["pill", "small-pill"])
            .margin_top(6)
            .build();
        let s = sender.clone();
        copy.connect_clicked(move |_| s.input(Input::Copy));
        let hero = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .css_classes(["overview-hero"])
            .build();
        for w in [logo.upcast_ref::<gtk::Widget>(), host.upcast_ref(), device.upcast_ref(), copy.upcast_ref()] {
            hero.append(w);
        }
        let group = adw::PreferencesGroup::new();
        group.add(&hero);
        root.add(&group);

        let hardware = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .row_spacing(10)
            .column_spacing(10)
            .css_classes(["spec-grid"])
            .build();
        let group = adw::PreferencesGroup::builder().title("Hardware").build();
        group.add(&hardware);
        root.add(&group);

        let software =
            gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        let group = adw::PreferencesGroup::builder().title("Software").build();
        group.add(&software);
        root.add(&group);

        read(&sender);
        let widgets = Widgets { logo, host, device, hardware, software };
        ComponentParts { model: Self { info: None }, widgets }
    }

    fn update(&mut self, input: Input, sender: ComponentSender<Self>, _: &Self::Root) {
        match input {
            Input::Visible(true) => read(&sender),
            Input::Visible(false) => {}
            Input::Copy => {
                if let (Some(info), Some(display)) = (&self.info, gdk::Display::default()) {
                    display.clipboard().set_text(&info.summary());
                    let _ = sender.output(Output::Toast("System info copied".into()));
                }
            }
        }
    }

    fn update_cmd_with_view(&mut self, w: &mut Widgets, cmd: Cmd, _: ComponentSender<Self>, _: &Self::Root) {
        let Cmd::Read(info) = cmd;
        show(w, &info);
        self.info = Some(*info);
    }
}

fn read(sender: &ComponentSender<OverviewPage>) {
    sender.oneshot_command(async {
        Cmd::Read(Box::new(tokio::task::spawn_blocking(sysinfo::read).await.unwrap_or_default()))
    });
}

fn show(w: &Widgets, info: &SystemInfo) {
    set_logo(&w.logo, info.os_logo.as_deref());
    w.host.set_label(&info.hostname);
    let device: Vec<String> = [info.device(), Some(info.os_name.clone())].into_iter().flatten().collect();
    w.device.set_label(&device.join(" · "));

    w.hardware.remove_all();
    if let Some(cpu) = &info.cpu {
        let detail = if info.threads != info.cores {
            format!("{} cores, {} threads", info.cores, info.threads)
        } else {
            format!("{} cores", info.cores)
        };
        w.hardware.append(&spec("dmm-cpu-symbolic", "Processor", cpu, &detail));
    }
    let memory_type = info.memory_type.clone().unwrap_or_default();
    w.hardware.append(&spec("dmm-memory-symbolic", "Memory", &sysinfo::memory(info.memory), &memory_type));
    for gpu in &info.gpus {
        w.hardware.append(&spec("dmm-gpu-symbolic", "Graphics", gpu, ""));
    }
    for d in &info.displays {
        let value = format!("{} × {}", d.width, d.height);
        let detail = format!("{:.0} Hz · scale {} · {}", d.refresh, trim_float(d.scale), d.name);
        w.hardware.append(&spec("dmm-video-display-symbolic", "Display", &value, &detail));
    }
    if let Some(disk) = &info.disk {
        let card = spec(
            "dmm-drive-harddisk-symbolic",
            "Storage",
            &format!("{} free", sysinfo::bytes(disk.total.saturating_sub(disk.used))),
            &format!("{} of {} used", sysinfo::bytes(disk.used), sysinfo::bytes(disk.total)),
        );
        let bar = gtk::LevelBar::builder()
            .min_value(0.0)
            .max_value(1.0)
            .value(disk.used as f64 / disk.total.max(1) as f64)
            .margin_top(4)
            .build();
        card.append(&bar);
        w.hardware.append(&card);
    }

    w.software.remove_all();
    let mut rows = vec![("Operating System", info.os_name.clone()), ("Kernel", info.kernel.clone())];
    rows.extend(info.compositor.clone().map(|c| ("Compositor", c)));
    rows.extend(info.shell.clone().map(|s| ("Shell", s)));
    rows.extend(info.packages.map(|p| ("Packages", p.to_string())));
    rows.push(("User", format!("{}@{}", info.user, info.hostname)));
    rows.push(("Uptime", sysinfo::duration(info.uptime)));
    rows.push(("DM Manager", env!("CARGO_PKG_VERSION").to_string()));
    for (title, value) in rows {
        let row = adw::ActionRow::builder().title(title).build();
        let label =
            gtk::Label::builder().label(&value).selectable(true).xalign(1.0).css_classes(["value-label"]).build();
        row.add_suffix(&label);
        w.software.append(&row);
    }
}

fn trim_float(n: f64) -> String {
    let s = format!("{n:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A card: icon and what it is, then the value and a detail line.
fn spec(icon: &str, title: &str, value: &str, detail: &str) -> gtk::Box {
    let head = gtk::Box::builder().spacing(10).build();
    head.append(&tile(icon));
    head.append(&gtk::Label::builder().label(title).xalign(0.0).css_classes(["dim-label", "caption-heading"]).build());
    let card =
        gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).css_classes(["spec-card"]).build();
    card.append(&head);
    card.append(
        &gtk::Label::builder().label(value).xalign(0.0).wrap(true).selectable(true).css_classes(["spec-value"]).build(),
    );
    if !detail.is_empty() {
        card.append(
            &gtk::Label::builder().label(detail).xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build(),
        );
    }
    card
}

/// The distribution's logo: an icon theme name, or a file where distributions put it.
fn set_logo(image: &gtk::Image, logo: Option<&str>) {
    let fallback = "dmm-computer-symbolic";
    let Some(logo) = logo else { return image.set_icon_name(Some(fallback)) };
    let theme = gdk::Display::default().map(|d| gtk::IconTheme::for_display(&d));
    if theme.is_some_and(|t| t.has_icon(logo)) {
        return image.set_icon_name(Some(logo));
    }
    let file = ["/usr/share/icons", "/usr/share/pixmaps"]
        .iter()
        .flat_map(|dir| ["svg", "png"].map(|ext| Path::new(dir).join(format!("{logo}.{ext}"))))
        .find(|p| p.exists());
    match file {
        Some(path) => image.set_from_file(Some(path)),
        None => image.set_icon_name(Some(fallback)),
    }
}
