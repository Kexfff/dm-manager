//! Default Apps: what opens links, folders, text, images, video, music, PDFs and archives
//! (GIO and `mimeapps.list`), and the terminal (Dolphin's "Open Terminal" and, if wanted,
//! niri's terminal shortcuts).

use dmm_sys::{apps, niri};
use relm4::adw::prelude::*;
use relm4::gtk::gio;
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::widgets::{Bindings, Hero, tile};

struct Category {
    title: &'static str,
    icon: &'static str,
    /// The type that decides the current default and the candidates.
    primary: &'static str,
    /// Every type the choice is applied to (where the app supports it).
    types: &'static [&'static str],
}

const CATEGORIES: &[Category] = &[
    Category {
        title: "Web Browser",
        icon: "dmm-web-browser-symbolic",
        primary: "x-scheme-handler/http",
        types: &["x-scheme-handler/http", "x-scheme-handler/https", "text/html", "application/xhtml+xml"],
    },
    Category {
        title: "Email",
        icon: "dmm-mail-unread-symbolic",
        primary: "x-scheme-handler/mailto",
        types: &["x-scheme-handler/mailto"],
    },
    Category { title: "Files", icon: "dmm-folder-symbolic", primary: "inode/directory", types: &["inode/directory"] },
    Category {
        title: "Text",
        icon: "dmm-text-editor-symbolic",
        primary: "text/plain",
        types: &["text/plain", "text/markdown", "text/x-log", "application/json", "application/x-yaml", "text/x-csrc"],
    },
    Category {
        title: "Images",
        icon: "dmm-image-x-generic-symbolic",
        primary: "image/png",
        types: &[
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/bmp",
            "image/tiff",
            "image/svg+xml",
            "image/avif",
            "image/heif",
            "image/jxl",
            "image/x-icon",
        ],
    },
    Category {
        title: "Video",
        icon: "dmm-video-x-generic-symbolic",
        primary: "video/mp4",
        types: &[
            "video/mp4",
            "video/x-matroska",
            "video/webm",
            "video/quicktime",
            "video/x-msvideo",
            "video/mpeg",
            "video/ogg",
        ],
    },
    Category {
        title: "Music",
        icon: "dmm-audio-x-generic-symbolic",
        primary: "audio/mpeg",
        types: &[
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/x-wav",
            "audio/mp4",
            "audio/aac",
            "audio/opus",
            "audio/x-vorbis+ogg",
        ],
    },
    Category {
        title: "PDF Documents",
        icon: "dmm-x-office-document-symbolic",
        primary: "application/pdf",
        types: &["application/pdf"],
    },
    Category {
        title: "Archives",
        icon: "dmm-package-x-generic-symbolic",
        primary: "application/zip",
        types: &[
            "application/zip",
            "application/x-tar",
            "application/gzip",
            "application/x-compressed-tar",
            "application/x-7z-compressed",
            "application/vnd.rar",
            "application/x-xz-compressed-tar",
            "application/zstd",
        ],
    },
];

pub struct AppsPage;

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Pick(usize, gio::AppInfo),
    Terminal(gio::AppInfo),
    /// Point niri's terminal shortcuts at this program.
    RebindTerminal {
        from: String,
        to: String,
    },
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    bindings: Bindings<()>,
}

impl Component for AppsPage {
    type CommandOutput = ();
    type Input = Input;
    type Output = Output;
    type Init = ();
    type Root = adw::PreferencesPage;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        adw::PreferencesPage::new()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let hero = Hero::new("dmm-view-app-grid-symbolic", "Default Apps");
        hero.set_subtitle("What opens your links and files");
        let hero_group = adw::PreferencesGroup::new();
        hero_group.add(&hero.root);
        root.add(&hero_group);

        let mut bindings = Bindings::new();
        let group = adw::PreferencesGroup::builder().title("Open With").build();
        for (index, category) in CATEGORIES.iter().enumerate() {
            let s = sender.clone();
            group.add(&app_row(
                category.title,
                category.icon,
                move || gio::AppInfo::all_for_type(category.primary),
                move || gio::AppInfo::default_for_type(category.primary, false).map(|a| app_id(&a)),
                move |app| s.input(Input::Pick(index, app)),
                &mut bindings,
            ));
        }
        root.add(&group);

        let group = adw::PreferencesGroup::builder()
            .title("Terminal")
            .description("Used by Dolphin's “Open Terminal” and by apps that start one")
            .build();
        let s = sender.clone();
        group.add(&app_row(
            "Terminal",
            "dmm-utilities-terminal-symbolic",
            terminals,
            apps::current_terminal,
            move |app| s.input(Input::Terminal(app)),
            &mut bindings,
        ));
        root.add(&group);

        let widgets = Widgets { bindings };
        widgets.bindings.refresh(&());
        ComponentParts { model: AppsPage, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => w.bindings.refresh(&()),
            Input::Visible(false) => {}
            Input::Pick(index, app) => {
                let category = &CATEGORIES[index];
                let supported = app.supported_types();
                let mut errors = Vec::new();
                for t in category.types {
                    if (*t == category.primary || supported.iter().any(|s| s == t))
                        && let Err(e) = app.set_as_default_for_type(t)
                    {
                        errors.push(e.to_string());
                    }
                }
                apps::refresh_kde_cache();
                toast(
                    &sender,
                    match errors.first() {
                        Some(e) => format!("Couldn't set {}: {e}", app.display_name()),
                        None => format!("{} now opens {}", app.display_name(), category.title.to_lowercase()),
                    },
                );
            }
            Input::Terminal(app) => {
                let old = apps::current_terminal()
                    .and_then(|id| gio_unix::DesktopAppInfo::new(&id))
                    .map(|a| program(a.upcast_ref()));
                let new = program(&app);
                if let Err(e) = apps::set_terminal(&app_id(&app), &new) {
                    toast(&sender, e);
                    return;
                }
                toast(&sender, format!("{} is the default terminal", app.display_name()));
                // Offer to move niri's shortcuts that start the old terminal.
                let Some(old) = old.filter(|o| *o != new.as_str()) else { return };
                let config = niri::effective();
                let keys: Vec<String> = config
                    .binds
                    .iter()
                    .filter(|(_, b)| b.spawn.as_deref().map(basename) == Some(basename(&old)))
                    .map(|(k, _)| k.clone())
                    .collect();
                if keys.is_empty() || !config.included {
                    return;
                }
                let dialog = adw::AlertDialog::new(
                    Some(&format!("Open {} With {}?", keys.join(" and "), app.display_name())),
                    Some(&format!("niri's {} start {old}.", if keys.len() > 1 { "shortcuts" } else { "shortcut" })),
                );
                dialog.add_responses(&[("keep", "Keep"), ("change", "Change")]);
                dialog.set_response_appearance("change", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("change"));
                dialog.set_close_response("keep");
                let s = sender.clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "change" {
                        s.input(Input::RebindTerminal { from: old.clone(), to: new.clone() });
                    }
                });
                dialog.present(Some(root));
            }
            Input::RebindTerminal { from, to } => {
                let config = niri::effective();
                let result = niri::NiriFile::load().and_then(|mut file| {
                    for (key, bind) in &config.binds {
                        if bind.spawn.as_deref().map(basename) == Some(basename(&from)) {
                            file.rebind_spawn(key, bind, &to)?;
                        }
                    }
                    file.save()
                });
                toast(
                    &sender,
                    match result {
                        Ok(()) => format!("niri's terminal shortcuts now open {to}"),
                        Err(e) => format!("Couldn't change the shortcuts: {e}"),
                    },
                );
            }
        }
        w.bindings.refresh(&());
    }
}

fn toast(sender: &ComponentSender<AppsPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

/// The desktop file ID (`org.kde.dolphin.desktop`).
fn app_id(app: &gio::AppInfo) -> String {
    app.id().map(|s| s.to_string()).unwrap_or_default()
}

/// The program an app runs (`kitty`).
fn program(app: &gio::AppInfo) -> String {
    app.executable().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Installed terminal emulators.
fn terminals() -> Vec<gio::AppInfo> {
    let mut list: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| {
            a.downcast_ref::<gio_unix::DesktopAppInfo>()
                .and_then(|d| d.categories())
                .is_some_and(|c| c.split(';').any(|c| c == "TerminalEmulator"))
                && a.should_show()
        })
        .collect();
    list.sort_by_key(|a| a.display_name().to_lowercase());
    list
}

/// A row choosing among apps (shown with their icons). `candidates` and `current` are read
/// again on every refresh, so apps installed meanwhile show up.
fn app_row(
    title: &str,
    icon: &str,
    candidates: impl Fn() -> Vec<gio::AppInfo> + 'static,
    current: impl Fn() -> Option<String> + 'static,
    on_pick: impl Fn(gio::AppInfo) + 'static,
    bindings: &mut Bindings<()>,
) -> adw::ComboRow {
    let store = gio::ListStore::new::<gio::AppInfo>();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let row = gtk::Box::builder().spacing(10).build();
        row.append(&gtk::Image::builder().pixel_size(22).build());
        row.append(&gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build());
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(app), Some(row)) = (item.item().and_downcast::<gio::AppInfo>(), item.child()) else { return };
        let image = row.first_child().and_downcast::<gtk::Image>().expect("image");
        let label = row.last_child().and_downcast::<gtk::Label>().expect("label");
        match app.icon() {
            Some(icon) => image.set_from_gicon(&icon),
            None => image.set_icon_name(Some("application-x-executable")),
        }
        label.set_label(&app.display_name());
    });
    let row = adw::ComboRow::builder().title(title).model(&store).factory(&factory).build();
    row.add_prefix(&tile(icon));

    let quiet = bindings.quiet();
    let s = store.clone();
    row.connect_selected_notify(move |r| {
        if quiet.get() {
            return;
        }
        if let Some(app) = s.item(r.selected()).and_downcast::<gio::AppInfo>() {
            on_pick(app);
        }
    });

    let r = row.clone();
    bindings.add(move |()| {
        let mut apps = candidates();
        apps.sort_by_key(|a| a.display_name().to_lowercase());
        apps.dedup_by_key(|a| app_id(a));
        let current = current();
        // The current default may not list the type itself (set by hand).
        if let Some(id) = &current
            && !apps.iter().any(|a| app_id(a) == *id)
            && let Some(app) = gio_unix::DesktopAppInfo::new(id)
        {
            apps.insert(0, app.upcast());
        }
        store.remove_all();
        for app in &apps {
            store.append(app);
        }
        let index = current.and_then(|id| apps.iter().position(|a| app_id(a) == id));
        r.set_selected(index.map_or(gtk::INVALID_LIST_POSITION, |i| i as u32));
        r.set_sensitive(!apps.is_empty());
        r.set_subtitle(match (apps.is_empty(), index) {
            (true, _) => "No installed app handles this",
            (false, None) => "Not set",
            _ => "",
        });
    });
    row
}
