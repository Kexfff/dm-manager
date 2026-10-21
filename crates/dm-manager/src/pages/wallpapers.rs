//! Wallpapers: explore Wallhaven and set or download what you like, or pick from the
//! wallpapers you have (`~/Pictures/Wallpapers`). Wallpapers are set with awww.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dmm_sys::wallhaven::{self, Client, Details, Query, Sorting, ThumbSize, Wallpaper};
use dmm_sys::{sysinfo, wallpaper};
use relm4::adw::prelude::*;
use relm4::gtk::{gdk_pixbuf, gio};
use relm4::{Component, ComponentParts, ComponentSender, adw, gtk};

use crate::settings;
use crate::widgets::icon_button;

/// Wallhaven's color palette (a subset), for searching by color.
const COLORS: &[&str] = &[
    "660000", "cc0000", "ea4c88", "993399", "663399", "333399", "0066cc", "0099cc", "66cccc", "77cc33", "336600",
    "cccc33", "ffcc33", "ff9900", "ff6600", "996633", "000000", "999999", "ffffff", "424153",
];
const RESOLUTIONS: &[(u32, u32)] = &[(1920, 1080), (2560, 1440), (3840, 2160)];
const RATIOS: &[(&str, &str)] =
    &[("16x9", "16:9"), ("16x10", "16:10"), ("21x9", "21:9"), ("32x9", "32:9"), ("9x16", "9:16 (portrait)")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Explore,
    Library,
}

pub struct WallpapersPage {
    client: Client,
    query: Query,
    results: Vec<Wallpaper>,
    last_page: u32,
    total: u64,
    loading: bool,
    /// Bumped with every new search, so late answers to an old one are dropped.
    generation: u64,
    started: bool,
    error: Option<String>,
    /// Downloads in progress, by wallpaper id: fraction done.
    downloads: HashMap<String, f64>,
    folder: PathBuf,
    library: Vec<PathBuf>,
    current: Vec<PathBuf>,
    mode: Mode,
    /// The wallpaper open in the detail dialog.
    open: Option<Open>,
}

#[derive(Debug, Clone)]
enum Open {
    Remote(Box<Wallpaper>),
    Local(PathBuf),
}

#[derive(Debug)]
pub enum Input {
    Visible(bool),
    Mode(Mode),
    Search(String),
    Sorting(Sorting),
    TopRange(&'static str),
    Category(usize, bool),
    Purity(usize, bool),
    AtLeast(Option<(u32, u32)>),
    Ratio(Option<&'static str>),
    Color(Option<String>),
    ApiKey(String),
    Retry,
    LoadMore,
    OpenResult(usize),
    OpenLocal(usize),
    /// Download (if needed) and set the open wallpaper.
    Set,
    Download,
    ShowOnline,
    ShowInFolder,
    Trash,
    SearchTag(String),
    DialogClosed,
}

#[derive(Debug)]
pub enum Cmd {
    Results { generation: u64, result: Result<wallhaven::Page, String> },
    Thumb { generation: u64, index: usize, path: Result<PathBuf, String> },
    LocalThumb { index: usize, path: Option<PathBuf> },
    Large { id: String, path: Result<PathBuf, String> },
    Details { id: String, result: Result<Details, String> },
    Progress { id: String, fraction: f64 },
    Downloaded { id: String, then_set: bool, result: Result<PathBuf, String> },
    Set(Result<PathBuf, String>),
}

#[derive(Debug)]
pub enum Output {
    Toast(String),
}

pub struct Widgets {
    modes: adw::ToggleGroup,
    stack: gtk::Stack,
    search: gtk::SearchEntry,
    explore_bar: gtk::Box,
    library_bar: gtk::Box,
    range: gtk::DropDown,
    nsfw: gtk::ToggleButton,
    grid: gtk::FlowBox,
    pictures: Vec<gtk::Picture>,
    footer: gtk::Stack,
    status: adw::StatusPage,
    results_stack: gtk::Stack,
    library_grid: gtk::FlowBox,
    library_pictures: Vec<gtk::Picture>,
    library_count: gtk::Label,
    library_status: gtk::Stack,
    dialog: Option<DetailWidgets>,
}

/// The parts of the open detail dialog that change.
struct DetailWidgets {
    dialog: adw::Dialog,
    picture: gtk::Picture,
    tags: gtk::FlowBox,
    set: gtk::Button,
    download: gtk::Button,
}

impl Component for WallpapersPage {
    type CommandOutput = Cmd;
    type Input = Input;
    type Output = Output;
    type Init = ();
    type Root = gtk::Box;
    type Widgets = Widgets;

    fn init_root() -> Self::Root {
        gtk::Box::builder().orientation(gtk::Orientation::Vertical).build()
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let api_key = settings::get("wallhaven-api-key");
        let client = Client::new(api_key.clone());

        // The bar: mode, then search and sorting (explore) or the folder (library).
        let modes = adw::ToggleGroup::builder().valign(gtk::Align::Center).build();
        modes.add(adw::Toggle::builder().name("explore").label("Explore").build());
        modes.add(adw::Toggle::builder().name("library").label("Downloaded").build());
        modes.set_active_name(Some("explore"));
        let s = sender.clone();
        modes.connect_active_name_notify(move |g| {
            s.input(Input::Mode(if g.active_name().as_deref() == Some("library") {
                Mode::Library
            } else {
                Mode::Explore
            }));
        });

        let search = gtk::SearchEntry::builder().placeholder_text("Search Wallhaven").search_delay(450).build();
        let s = sender.clone();
        search.connect_search_changed(move |e| s.input(Input::Search(e.text().to_string())));

        let sortings: Vec<&str> = Sorting::ALL.iter().map(|s| s.label()).collect();
        let sorting = gtk::DropDown::from_strings(&sortings);
        sorting.set_valign(gtk::Align::Center);
        let s = sender.clone();
        sorting.connect_selected_notify(move |d| {
            if let Some(sort) = Sorting::ALL.get(d.selected() as usize) {
                s.input(Input::Sorting(*sort));
            }
        });
        let ranges: Vec<&str> = wallhaven::TOP_RANGES.iter().map(|(_, l)| *l).collect();
        let range = gtk::DropDown::from_strings(&ranges);
        range.set_valign(gtk::Align::Center);
        range.set_selected(3);
        let s = sender.clone();
        range.connect_selected_notify(move |d| {
            if let Some((param, _)) = wallhaven::TOP_RANGES.get(d.selected() as usize) {
                s.input(Input::TopRange(param));
            }
        });

        let (filters, nsfw) = filters_button(&sender, api_key.as_deref());
        let explore_bar = gtk::Box::builder().spacing(8).build();
        for w in [sorting.upcast_ref::<gtk::Widget>(), range.upcast_ref(), filters.upcast_ref()] {
            explore_bar.append(w);
        }

        let library_count = gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).visible(false).build();
        let open_folder = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder().icon_name("dmm-folder-pictures-symbolic").label("Open Folder").build(),
            )
            .css_classes(["flat"])
            .build();
        let s = sender.clone();
        open_folder.connect_clicked(move |_| s.input(Input::ShowInFolder));
        let library_bar = gtk::Box::builder().spacing(8).visible(false).build();
        library_bar.append(&open_folder);

        // Mode and sorting on top, the search (or the folder) below.
        let top = gtk::Box::builder().spacing(8).build();
        top.append(&modes);
        top.append(&gtk::Box::builder().hexpand(true).build());
        top.append(&explore_bar);
        top.append(&library_bar);
        let bar =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(10).css_classes(["wall-bar"]).build();
        bar.append(&top);
        bar.append(&search);
        bar.append(&library_count);
        root.append(&adw::Clamp::builder().maximum_size(1400).child(&bar).build());
        let modes = modes.clone();

        // Explore: a grid that loads more as you reach the end.
        let grid = new_grid();
        let s = sender.clone();
        grid.connect_child_activated(move |_, child| s.input(Input::OpenResult(child.index() as usize)));
        let footer = gtk::Stack::builder().margin_top(12).margin_bottom(24).build();
        footer.add_named(&adw::Spinner::builder().width_request(32).height_request(32).build(), Some("loading"));
        let more = gtk::Button::builder().label("Load More").halign(gtk::Align::Center).css_classes(["pill"]).build();
        let s = sender.clone();
        more.connect_clicked(move |_| s.input(Input::LoadMore));
        footer.add_named(&more, Some("more"));
        footer.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("end"));
        let column = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
        column.append(&grid);
        column.append(&footer);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(1400).child(&column).build())
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let s = sender.clone();
        scroller.connect_edge_reached(move |_, edge| {
            if edge == gtk::PositionType::Bottom {
                s.input(Input::LoadMore);
            }
        });
        let retry = gtk::Button::builder().label("Try Again").halign(gtk::Align::Center).css_classes(["pill"]).build();
        let s = sender.clone();
        retry.connect_clicked(move |_| s.input(Input::Retry));
        let status =
            adw::StatusPage::builder().icon_name("dmm-preferences-desktop-wallpaper-symbolic").child(&retry).build();
        let results_stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).build();
        results_stack.add_named(&scroller, Some("grid"));
        results_stack.add_named(&status, Some("status"));

        // Library
        let library_grid = new_grid();
        let s = sender.clone();
        library_grid.connect_child_activated(move |_, child| s.input(Input::OpenLocal(child.index() as usize)));
        let library_scroller = gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(1400).child(&library_grid).build())
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("dmm-folder-pictures-symbolic")
            .title("No Wallpapers Yet")
            .description("Wallpapers you download land in ~/Pictures/Wallpapers")
            .build();
        let library_status = gtk::Stack::new();
        library_status.add_named(&library_scroller, Some("grid"));
        library_status.add_named(&empty, Some("empty"));

        let stack = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).vexpand(true).build();
        stack.add_named(&results_stack, Some("explore"));
        stack.add_named(&library_status, Some("library"));
        root.append(&stack);

        let model = Self {
            client,
            query: Query::default(),
            results: Vec::new(),
            last_page: 0,
            total: 0,
            loading: false,
            generation: 0,
            started: false,
            error: None,
            downloads: HashMap::new(),
            folder: wallpaper::folder(),
            library: Vec::new(),
            current: Vec::new(),
            mode: Mode::Explore,
            open: None,
        };
        let widgets = Widgets {
            modes,
            stack,
            search,
            explore_bar,
            library_bar,
            range,
            nsfw,
            grid,
            pictures: Vec::new(),
            footer,
            status,
            results_stack,
            library_grid,
            library_pictures: Vec::new(),
            library_count,
            library_status,
            dialog: None,
        };
        ComponentParts { model, widgets }
    }

    fn update_with_view(&mut self, w: &mut Widgets, input: Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match input {
            Input::Visible(true) => {
                // Nothing goes to Wallhaven until the page is first opened.
                if !self.started {
                    self.started = true;
                    self.search(w, &sender);
                }
                self.refresh_library(w, &sender);
            }
            Input::Visible(false) => {}
            Input::Mode(mode) => {
                self.mode = mode;
                if mode == Mode::Library {
                    self.refresh_library(w, &sender);
                }
            }
            Input::Search(text) => {
                self.query.text = text;
                self.search(w, &sender);
            }
            Input::Sorting(sorting) => {
                self.query.sorting = sorting;
                self.search(w, &sender);
            }
            Input::TopRange(range) => {
                self.query.top_range = range;
                self.search(w, &sender);
            }
            Input::Category(i, on) => {
                self.query.categories[i] = on;
                self.search(w, &sender);
            }
            Input::Purity(i, on) => {
                self.query.purity[i] = on;
                self.search(w, &sender);
            }
            Input::AtLeast(size) => {
                self.query.at_least = size;
                self.search(w, &sender);
            }
            Input::Ratio(ratio) => {
                self.query.ratios = ratio.map(|r| vec![r.to_string()]).unwrap_or_default();
                self.search(w, &sender);
            }
            Input::Color(color) => {
                self.query.color = color;
                self.search(w, &sender);
            }
            Input::ApiKey(key) => {
                let key = key.trim().to_string();
                settings::set("wallhaven-api-key", (!key.is_empty()).then_some(key.as_str()));
                self.client = Client::new((!key.is_empty()).then_some(key));
                if !self.client.has_api_key() {
                    self.query.purity[2] = false;
                    w.nsfw.set_active(false);
                }
                w.nsfw.set_sensitive(self.client.has_api_key());
                self.search(w, &sender);
            }
            Input::Retry => self.search(w, &sender),
            Input::LoadMore => {
                if !self.loading && self.started && self.query.page < self.last_page && self.error.is_none() {
                    self.query.page += 1;
                    self.fetch(&sender);
                }
            }
            Input::OpenResult(index) => {
                if let Some(wp) = self.results.get(index).cloned() {
                    self.open = Some(Open::Remote(Box::new(wp.clone())));
                    w.dialog = Some(detail_dialog(root, &sender, &Open::Remote(Box::new(wp.clone()))));
                    let client = self.client.clone();
                    let id = wp.id.clone();
                    let wp2 = wp.clone();
                    sender.oneshot_command(async move {
                        Cmd::Large { id: wp2.id.clone(), path: client.thumbnail(&wp2, ThumbSize::Large).await }
                    });
                    let client = self.client.clone();
                    sender.oneshot_command(async move { Cmd::Details { result: client.details(&id).await, id } });
                }
            }
            Input::OpenLocal(index) => {
                if let Some(path) = self.library.get(index).cloned() {
                    let open = Open::Local(path);
                    w.dialog = Some(detail_dialog(root, &sender, &open));
                    self.open = Some(open);
                }
            }
            Input::Set => match self.open.clone() {
                Some(Open::Remote(wp)) => self.download(&sender, *wp, true),
                Some(Open::Local(path)) => set_wallpaper(&sender, path),
                None => {}
            },
            Input::Download => {
                if let Some(Open::Remote(wp)) = self.open.clone() {
                    self.download(&sender, *wp, false);
                }
            }
            Input::ShowOnline => {
                if let Some(Open::Remote(wp)) = &self.open {
                    launch(&wp.url);
                }
            }
            Input::ShowInFolder => {
                let _ = std::fs::create_dir_all(&self.folder);
                let target = match &self.open {
                    Some(Open::Local(path)) if w.dialog.is_some() => path.clone(),
                    _ => self.folder.clone(),
                };
                show_in_folder(&target);
            }
            Input::Trash => {
                if let Some(Open::Local(path)) = self.open.clone() {
                    match gio::File::for_path(&path).trash(gio::Cancellable::NONE) {
                        Ok(()) => {
                            toast(&sender, "Moved to the trash".into());
                            if let Some(d) = w.dialog.take() {
                                d.dialog.close();
                            }
                            self.refresh_library(w, &sender);
                        }
                        Err(e) => toast(&sender, format!("Couldn't move it to the trash: {e}")),
                    }
                }
            }
            Input::SearchTag(text) => {
                if let Some(d) = w.dialog.take() {
                    d.dialog.close();
                }
                self.query = Query { text, ..self.query.clone() };
                self.query.color = None;
                self.mode = Mode::Explore;
                self.search(w, &sender);
            }
            Input::DialogClosed => {
                w.dialog = None;
                self.open = None;
            }
        }
        self.show(w);
    }

    fn update_cmd_with_view(&mut self, w: &mut Widgets, cmd: Cmd, sender: ComponentSender<Self>, _: &Self::Root) {
        match cmd {
            Cmd::Results { generation, result } => {
                if generation != self.generation {
                    return;
                }
                self.loading = false;
                match result {
                    Ok(page) => {
                        self.error = None;
                        self.last_page = page.last_page;
                        self.total = page.total;
                        if page.seed.is_some() {
                            self.query.seed = page.seed;
                        }
                        let start = self.results.len();
                        for (i, wp) in page.wallpapers.iter().enumerate() {
                            let picture = add_card(&w.grid, &wp.resolution, false);
                            w.pictures.push(picture);
                            let (client, wp, generation) = (self.client.clone(), wp.clone(), self.generation);
                            sender.oneshot_command(async move {
                                Cmd::Thumb {
                                    generation,
                                    index: start + i,
                                    path: client.thumbnail(&wp, ThumbSize::Small).await,
                                }
                            });
                        }
                        self.results.extend(page.wallpapers);
                    }
                    Err(e) => {
                        // A failed "load more" keeps what's shown.
                        if self.results.is_empty() {
                            self.error = Some(e);
                        } else {
                            self.query.page -= 1;
                            toast(&sender, e);
                        }
                    }
                }
            }
            Cmd::Thumb { generation, index, path } => {
                if let (true, Ok(path), Some(picture)) = (generation == self.generation, path, w.pictures.get(index)) {
                    picture.set_filename(Some(&path));
                    picture.add_css_class("loaded");
                }
            }
            Cmd::LocalThumb { index, path } => {
                if let (Some(path), Some(picture)) = (path, w.library_pictures.get(index)) {
                    picture.set_filename(Some(&path));
                    picture.add_css_class("loaded");
                }
            }
            Cmd::Large { id, path } => {
                if let (Some(Open::Remote(wp)), Some(d), Ok(path)) = (&self.open, &w.dialog, path)
                    && wp.id == id
                {
                    d.picture.set_filename(Some(&path));
                }
            }
            Cmd::Details { id, result } => {
                if let (Some(Open::Remote(wp)), Some(d)) = (&self.open, &w.dialog)
                    && wp.id == id
                {
                    for tag in result.map(|r| r.tags).unwrap_or_default() {
                        let button = gtk::Button::builder().label(&tag.name).css_classes(["tag"]).build();
                        let (s, name) = (sender.clone(), tag.name.clone());
                        button.connect_clicked(move |_| s.input(Input::SearchTag(name.clone())));
                        d.tags.append(&button);
                    }
                }
            }
            Cmd::Progress { id, fraction } => {
                self.downloads.insert(id, fraction);
            }
            Cmd::Downloaded { id, then_set, result } => {
                self.downloads.remove(&id);
                match result {
                    Ok(path) => {
                        self.refresh_library(w, &sender);
                        if then_set {
                            set_wallpaper(&sender, path);
                        } else {
                            toast(&sender, "Saved to your wallpapers".into());
                        }
                    }
                    Err(e) => toast(&sender, format!("Download failed: {e}")),
                }
            }
            Cmd::Set(result) => match result {
                Ok(path) => {
                    self.current = wallpaper::current();
                    if self.current.is_empty() {
                        self.current = vec![path];
                    }
                    toast(&sender, "Wallpaper set".into());
                    self.refresh_library(w, &sender);
                }
                Err(e) => toast(&sender, format!("Couldn't set the wallpaper: {e}")),
            },
        }
        self.show(w);
    }
}

impl WallpapersPage {
    /// Starts a new search from page 1, clearing the grid.
    fn search(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        self.generation += 1;
        self.query.page = 1;
        self.query.seed = None;
        self.results.clear();
        self.error = None;
        w.grid.remove_all();
        w.pictures.clear();
        self.fetch(sender);
    }

    fn fetch(&mut self, sender: &ComponentSender<Self>) {
        self.loading = true;
        let (client, query, generation) = (self.client.clone(), self.query.clone(), self.generation);
        sender.oneshot_command(async move { Cmd::Results { generation, result: client.search(&query).await } });
    }

    fn download(&mut self, sender: &ComponentSender<Self>, wp: Wallpaper, then_set: bool) {
        if self.downloads.contains_key(&wp.id) {
            return;
        }
        self.downloads.insert(wp.id.clone(), 0.0);
        let (client, folder) = (self.client.clone(), self.folder.clone());
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let id = wp.id.clone();
                    let last = std::sync::atomic::AtomicU32::new(0);
                    let progress = |done: u64, total: u64| {
                        let percent = (done * 100 / total.max(1)) as u32;
                        if percent != last.swap(percent, std::sync::atomic::Ordering::Relaxed) {
                            let _ = out.send(Cmd::Progress { id: id.clone(), fraction: f64::from(percent) / 100.0 });
                        }
                    };
                    let result = client.download(&wp, &folder, progress).await;
                    let _ = out.send(Cmd::Downloaded { id, then_set, result });
                })
                .drop_on_shutdown()
        });
    }

    fn refresh_library(&mut self, w: &mut Widgets, sender: &ComponentSender<Self>) {
        let library = wallpaper::library(&self.folder);
        self.current = wallpaper::current();
        if library != self.library || w.library_pictures.len() != library.len() {
            w.library_grid.remove_all();
            w.library_pictures.clear();
            for (index, path) in library.iter().enumerate() {
                let current = self.current.contains(path);
                w.library_pictures.push(add_card(&w.library_grid, "", current));
                let path = path.clone();
                sender.oneshot_command(async move {
                    Cmd::LocalThumb {
                        index,
                        path: tokio::task::spawn_blocking(move || local_thumbnail(&path)).await.ok().flatten(),
                    }
                });
            }
            self.library = library;
        } else {
            // Only the "current" badges may have moved.
            for (i, path) in self.library.iter().enumerate() {
                if let Some(badge) = w.library_grid.child_at_index(i as i32).and_then(current_badge) {
                    badge.set_visible(self.current.contains(path));
                }
            }
        }
    }

    fn show(&self, w: &mut Widgets) {
        let mode = if self.mode == Mode::Library { "library" } else { "explore" };
        w.stack.set_visible_child_name(mode);
        if w.modes.active_name().as_deref() != Some(mode) {
            w.modes.set_active_name(Some(mode));
        }
        w.explore_bar.set_visible(self.mode == Mode::Explore);
        w.library_bar.set_visible(self.mode == Mode::Library);
        w.search.set_visible(self.mode == Mode::Explore);
        w.library_count.set_visible(self.mode == Mode::Library);
        w.range.set_visible(self.query.sorting == Sorting::Toplist);

        match &self.error {
            Some(e) => {
                w.status.set_title("Can't Load Wallpapers");
                w.status.set_description(Some(e));
                w.status.child().inspect(|c| c.set_visible(true));
                w.results_stack.set_visible_child_name("status");
            }
            None if !self.loading && self.results.is_empty() && self.started => {
                w.status.set_title("No Wallpapers Found");
                w.status.set_description(Some("Try other words or filters"));
                w.status.child().inspect(|c| c.set_visible(false));
                w.results_stack.set_visible_child_name("status");
            }
            None => w.results_stack.set_visible_child_name("grid"),
        }
        w.footer.set_visible_child_name(if self.loading {
            "loading"
        } else if self.query.page < self.last_page {
            "more"
        } else {
            "end"
        });

        let n = self.library.len();
        w.library_count.set_label(&format!(
            "{n} wallpaper{} in {}",
            if n == 1 { "" } else { "s" },
            tilde(&self.folder)
        ));
        w.library_status.set_visible_child_name(if n == 0 { "empty" } else { "grid" });

        if let (Some(d), Some(open)) = (&w.dialog, &self.open) {
            match open {
                Open::Remote(wp) => {
                    let downloaded = self.folder.join(wp.file_name()).exists();
                    match self.downloads.get(&wp.id) {
                        Some(fraction) => {
                            d.set.set_label(&format!("Downloading… {:.0}%", fraction * 100.0));
                            d.set.set_sensitive(false);
                            d.download.set_sensitive(false);
                        }
                        None => {
                            d.set.set_label("Set as Wallpaper");
                            d.set.set_sensitive(true);
                            d.download.set_sensitive(!downloaded);
                            d.download.set_label(if downloaded { "Downloaded" } else { "Download" });
                        }
                    }
                }
                Open::Local(path) => {
                    let current = self.current.contains(path);
                    d.set.set_label(if current { "Current Wallpaper" } else { "Set as Wallpaper" });
                    d.set.set_sensitive(!current);
                }
            }
        }
    }
}

fn toast(sender: &ComponentSender<WallpapersPage>, text: String) {
    let _ = sender.output(Output::Toast(text));
}

fn set_wallpaper(sender: &ComponentSender<WallpapersPage>, path: PathBuf) {
    sender.oneshot_command(async move {
        let result = tokio::task::spawn_blocking(move || wallpaper::set(&path).map(|()| path)).await;
        Cmd::Set(result.unwrap_or_else(|e| Err(e.to_string())))
    });
}

fn launch(uri: &str) {
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, gio::AppLaunchContext::NONE) {
        log::warn!("can't open {uri}: {e}");
    }
}

/// Opens the folder in the file manager, with the file selected if it's a file.
fn show_in_folder(path: &Path) {
    let uri = gio::File::for_path(path).uri();
    if path.is_file() {
        // The FileManager1 interface selects the file (Dolphin, Nautilus…).
        let shown = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .status()
            .is_ok_and(|s| s.success());
        if shown {
            return;
        }
        if let Some(dir) = path.parent() {
            return launch(&gio::File::for_path(dir).uri());
        }
    }
    launch(&uri);
}

fn tilde(path: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let p = path.to_string_lossy();
    match p.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => p.into_owned(),
    }
}

/// A small copy of a local image for the grid (cached on disk).
fn local_thumbnail(image: &Path) -> Option<PathBuf> {
    let thumb = wallpaper::thumbnail_path(image);
    if thumb.exists() {
        return Some(thumb);
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(image, 560, 560, true).ok()?;
    std::fs::create_dir_all(thumb.parent()?).ok()?;
    pixbuf.savev(&thumb, "png", &[]).ok()?;
    Some(thumb)
}

fn new_grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(true)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(6)
        .row_spacing(12)
        .column_spacing(12)
        .valign(gtk::Align::Start)
        .css_classes(["wall-grid"])
        .build()
}

/// A card in a grid: the picture (filled in when its thumbnail arrives), a resolution chip,
/// and a "current" badge.
fn add_card(grid: &gtk::FlowBox, chip: &str, current: bool) -> gtk::Picture {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .width_request(220)
        .height_request(138)
        .build();
    let overlay =
        gtk::Overlay::builder().child(&picture).overflow(gtk::Overflow::Hidden).css_classes(["wall-card"]).build();
    if !chip.is_empty() {
        let label = gtk::Label::builder()
            .label(chip)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::End)
            .css_classes(["wall-chip"])
            .build();
        overlay.add_overlay(&label);
    }
    let badge = gtk::Label::builder()
        .label("Current")
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .visible(current)
        .css_classes(["wall-badge"])
        .name("current")
        .build();
    overlay.add_overlay(&badge);
    grid.append(&overlay);
    picture
}

fn current_badge(child: gtk::FlowBoxChild) -> Option<gtk::Widget> {
    let overlay = child.child()?;
    let mut next = overlay.first_child();
    while let Some(w) = next {
        if w.widget_name() == "current" {
            return Some(w);
        }
        next = w.next_sibling();
    }
    None
}

/// The filters popover: categories, content, size, ratio, color and the API key.
fn filters_button(
    sender: &ComponentSender<WallpapersPage>,
    api_key: Option<&str>,
) -> (gtk::MenuButton, gtk::ToggleButton) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(320)
        .build();
    let heading = |text: &str| gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).build();

    content.append(&heading("Categories"));
    let row = gtk::Box::builder().css_classes(["linked"]).homogeneous(true).build();
    for (i, (label, on)) in [("General", true), ("Anime", true), ("People", false)].into_iter().enumerate() {
        let t = gtk::ToggleButton::builder().label(label).active(on).build();
        let s = sender.clone();
        t.connect_toggled(move |b| s.input(Input::Category(i, b.is_active())));
        row.append(&t);
    }
    content.append(&row);

    content.append(&heading("Content"));
    let row = gtk::Box::builder().css_classes(["linked"]).homogeneous(true).build();
    let mut nsfw = None;
    for (i, label) in ["SFW", "Sketchy", "NSFW"].into_iter().enumerate() {
        let t = gtk::ToggleButton::builder().label(label).active(i == 0).build();
        let s = sender.clone();
        t.connect_toggled(move |b| s.input(Input::Purity(i, b.is_active())));
        row.append(&t);
        if i == 2 {
            t.set_sensitive(api_key.is_some());
            t.set_tooltip_text(Some("Needs a Wallhaven API key"));
            nsfw = Some(t);
        }
    }
    content.append(&row);

    content.append(&heading("At Least"));
    let mut sizes = vec!["Any Size".to_string()];
    sizes.extend(RESOLUTIONS.iter().map(|(w, h)| format!("{w} × {h}")));
    let screen = sysinfo::displays().into_iter().max_by_key(|d| d.width * d.height).map(|d| (d.width, d.height));
    if let Some((w, h)) = screen {
        sizes.push(format!("My Screen ({w} × {h})"));
    }
    let size_labels: Vec<&str> = sizes.iter().map(String::as_str).collect();
    let size = gtk::DropDown::from_strings(&size_labels);
    let s = sender.clone();
    size.connect_selected_notify(move |d| {
        let i = d.selected() as usize;
        let value = match i {
            0 => None,
            i if i <= RESOLUTIONS.len() => Some(RESOLUTIONS[i - 1]),
            _ => screen,
        };
        s.input(Input::AtLeast(value));
    });
    content.append(&size);

    content.append(&heading("Aspect Ratio"));
    let mut ratio_labels = vec!["Any Ratio"];
    ratio_labels.extend(RATIOS.iter().map(|(_, l)| *l));
    let ratio = gtk::DropDown::from_strings(&ratio_labels);
    let s = sender.clone();
    ratio.connect_selected_notify(move |d| {
        let value = (d.selected() as usize).checked_sub(1).and_then(|i| RATIOS.get(i)).map(|(r, _)| *r);
        s.input(Input::Ratio(value));
    });
    content.append(&ratio);

    content.append(&heading("Color"));
    let colors = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(10)
        .min_children_per_line(10)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    let any = gtk::ToggleButton::builder()
        .active(true)
        .tooltip_text("Any color")
        .css_classes(["swatch", "small", "any-color"])
        .build();
    let s = sender.clone();
    any.connect_toggled(move |b| {
        if b.is_active() {
            s.input(Input::Color(None));
        }
    });
    colors.append(&any);
    for color in COLORS {
        let t = gtk::ToggleButton::builder().tooltip_text(format!("#{color}")).css_classes(["swatch", "small"]).build();
        t.set_group(Some(&any));
        // Each swatch paints itself; a CSS class per color would need a stylesheet each.
        let rgba = gtk::gdk::RGBA::parse(format!("#{color}")).unwrap_or(gtk::gdk::RGBA::BLACK);
        let area = gtk::DrawingArea::builder().content_width(18).content_height(18).build();
        area.set_draw_func(move |_, cr, w, h| {
            cr.arc(f64::from(w) / 2.0, f64::from(h) / 2.0, f64::from(w.min(h)) / 2.0, 0.0, std::f64::consts::TAU);
            cr.set_source_rgba(f64::from(rgba.red()), f64::from(rgba.green()), f64::from(rgba.blue()), 1.0);
            let _ = cr.fill();
        });
        t.set_child(Some(&area));
        let s = sender.clone();
        t.connect_toggled(move |b| {
            if b.is_active() {
                s.input(Input::Color(Some(color.to_string())));
            }
        });
        colors.append(&t);
    }
    content.append(&colors);

    content.append(&heading("API Key"));
    let key = gtk::PasswordEntry::builder()
        .placeholder_text("Optional: for NSFW and your account's filters")
        .show_peek_icon(true)
        .text(api_key.unwrap_or_default())
        .build();
    let s = sender.clone();
    key.connect_activate(move |e| s.input(Input::ApiKey(e.text().to_string())));
    let k = key.clone();
    let s = sender.clone();
    key.connect_has_focus_notify(move |_| {
        if !k.has_focus() {
            s.input(Input::ApiKey(k.text().to_string()));
        }
    });
    content.append(&key);
    let link = gtk::LinkButton::builder()
        .label("Get one in your Wallhaven account settings")
        .uri("https://wallhaven.cc/settings/account")
        .halign(gtk::Align::Start)
        .build();
    content.append(&link);

    let popover = gtk::Popover::builder().child(&content).build();
    let button = gtk::MenuButton::builder()
        .icon_name("dmm-preferences-system-symbolic")
        .tooltip_text("Filters")
        .popover(&popover)
        .valign(gtk::Align::Center)
        .build();
    (button, nsfw.expect("three purity toggles"))
}

/// The dialog for one wallpaper: a big preview, what it is, and what to do with it.
fn detail_dialog(parent: &gtk::Box, sender: &ComponentSender<WallpapersPage>, open: &Open) -> DetailWidgets {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .vexpand(true)
        .css_classes(["detail-picture"])
        .build();
    let info = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(10).hexpand(true).build();
    let chips = gtk::Box::builder().spacing(6).build();
    let tags = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(12)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    let set = gtk::Button::builder().label("Set as Wallpaper").css_classes(["pill", "suggested-action"]).build();
    let download = gtk::Button::builder().label("Download").css_classes(["pill"]).build();
    let s = sender.clone();
    set.connect_clicked(move |_| s.input(Input::Set));
    let s = sender.clone();
    download.connect_clicked(move |_| s.input(Input::Download));
    let actions =
        gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).valign(gtk::Align::Center).build();
    actions.append(&set);

    let title = match open {
        Open::Remote(wp) => {
            for text in [
                wp.category.clone(),
                wp.purity.to_uppercase(),
                sysinfo::bytes(wp.file_size),
                format!("{} views", compact(wp.views)),
                format!("{} favorites", compact(wp.favorites)),
            ] {
                chips.append(&gtk::Label::builder().label(&text).css_classes(["wall-info-chip"]).build());
            }
            info.append(&chips);
            let palette = gtk::Box::builder().spacing(6).build();
            for color in &wp.colors {
                let button = gtk::Button::builder()
                    .tooltip_text(format!("More in {color}"))
                    .css_classes(["swatch", "small"])
                    .build();
                let rgba = gtk::gdk::RGBA::parse(color).unwrap_or(gtk::gdk::RGBA::BLACK);
                let area = gtk::DrawingArea::builder().content_width(18).content_height(18).build();
                area.set_draw_func(move |_, cr, w, h| {
                    cr.arc(
                        f64::from(w) / 2.0,
                        f64::from(h) / 2.0,
                        f64::from(w.min(h)) / 2.0,
                        0.0,
                        std::f64::consts::TAU,
                    );
                    cr.set_source_rgba(f64::from(rgba.red()), f64::from(rgba.green()), f64::from(rgba.blue()), 1.0);
                    let _ = cr.fill();
                });
                button.set_child(Some(&area));
                let (s, c) = (sender.clone(), color.trim_start_matches('#').to_string());
                button.connect_clicked(move |_| {
                    s.input(Input::Color(Some(c.clone())));
                    s.input(Input::SearchTag(String::new()));
                });
                palette.append(&button);
            }
            info.append(&palette);
            info.append(&tags);
            actions.append(&download);
            let online = gtk::Button::builder().label("Open on Wallhaven").css_classes(["flat"]).build();
            let s = sender.clone();
            online.connect_clicked(move |_| s.input(Input::ShowOnline));
            actions.append(&online);
            // The small thumbnail is cached already; show it until the large one arrives.
            picture.set_filename(Some(wallhaven::cache_dir().join(format!("{}-small.jpg", wp.id))));
            format!("{} × {}", wp.dimension_x, wp.dimension_y)
        }
        Open::Local(path) => {
            picture.set_filename(Some(path));
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(path).map(|m| sysinfo::bytes(m.len())).unwrap_or_default();
            let dims = gdk_pixbuf::Pixbuf::file_info(path).map(|(_, w, h)| format!("{w} × {h}")).unwrap_or_default();
            for text in [dims, size] {
                if !text.is_empty() {
                    chips.append(&gtk::Label::builder().label(&text).css_classes(["wall-info-chip"]).build());
                }
            }
            info.append(&gtk::Label::builder().label(&name).xalign(0.0).wrap(true).css_classes(["dim-label"]).build());
            info.append(&chips);
            let folder = gtk::Button::builder().label("Show in Folder").css_classes(["pill"]).build();
            let s = sender.clone();
            folder.connect_clicked(move |_| s.input(Input::ShowInFolder));
            actions.append(&folder);
            let trash = icon_button("dmm-user-trash-symbolic", "Move to trash");
            trash.set_halign(gtk::Align::Center);
            let s = sender.clone();
            trash.connect_clicked(move |_| s.input(Input::Trash));
            actions.append(&trash);
            name
        }
    };

    let bottom =
        gtk::Box::builder().spacing(18).margin_top(14).margin_bottom(18).margin_start(18).margin_end(18).build();
    bottom.append(&info);
    bottom.append(&actions);
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    let frame = gtk::Box::builder().margin_start(18).margin_end(18).vexpand(true).build();
    frame.append(&picture);
    body.append(&frame);
    body.append(&bottom);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new(&title, "")).build());
    view.set_content(Some(&body));
    let dialog = adw::Dialog::builder().content_width(960).content_height(720).child(&view).build();
    let s = sender.clone();
    dialog.connect_closed(move |_| s.input(Input::DialogClosed));
    dialog.present(Some(parent));
    DetailWidgets { dialog, picture, tags, set, download }
}

/// 12345 → "12K"
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1000..=999_999 => format!("{:.0}K", n as f64 / 1000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}
