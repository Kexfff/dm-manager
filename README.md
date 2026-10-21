# DM Manager

Settings and system tools for the [DM-Shell](https://github.com/Kexfff/DM-Shell) desktop on
[niri](https://github.com/YaLTeR/niri): one app for what a bare niri setup has no UI for.
It uses the dmsh look: near-black surfaces, Inter, rounded pills, and an accent color you pick.

Built with Rust, GTK 4 and libadwaita ([relm4](https://relm4.org)).

## What's in it

| Page | Status |
|---|---|
| **Wi-Fi** — on/off, current network with IP, router, band and link speed, nearby networks, joining (WPA2/WPA3/WEP/open, password prompt, retry on a wrong password), saved networks and forgetting them | ✅ |
| **Bluetooth** — on/off (unblocks rfkill), paired devices with battery level, connect/disconnect/forget, nearby devices and pairing, including passkey and PIN prompts | ✅ |
| **Shell** — everything in dmsh's config: colors and font, island sizes, clock formats, motion, timing, which displays show the islands, features and the session menu, with a live preview of the islands; restart dmsh from here | ✅ |
| **Keyboard & Input** — keyboard layouts and the shortcut that switches them, key repeat, Num Lock, touchpad and mouse (tap, natural scrolling, speed, acceleration…), cursor theme (with previews) and size | ✅ |
| **Default Apps** — browser, email, files, text, images, video, music, PDFs, archives (shown with app icons), and the terminal, including Dolphin's "Open Terminal" and, if you like, niri's terminal shortcuts | ✅ |
| **Appearance** — for Qt/KDE apps like Dolphin: color scheme (with previews), accent color (which also recolors Breeze folder icons, previewed), widget style, Kvantum theme; for GTK apps: light/dark, accent, GTK 3 theme, and switching off a theming tool's `gtk.css`; for both: icon theme (with previews) and fonts | ✅ |
| **Power & Sleep** — power mode, battery status, when the screen turns off, locks and the computer sleeps, and locking before sleep | ✅ |
| **Overview** — the page it opens on: the computer's name and logo, processor, memory, graphics, displays, storage, and the OS, kernel, niri and dmsh versions; copy it all for a bug report | ✅ |
| **Wallpapers** — explore Wallhaven (search, top/hot/latest/random…, categories, minimum resolution, aspect ratio, color; an optional API key unlocks NSFW), see a wallpaper's details and tags, and set or download it in one click; or pick from the ones in `~/Pictures/Wallpapers`. Set with awww | ✅ |
| Assistant, System Monitor | later |

Pick the accent from the colored dot at the top of the sidebar. It's saved in
`~/.config/dm-manager/settings.kdl`.

dm-manager keeps its own look even when a theming tool writes colors into
`~/.config/gtk-4.0/gtk.css` for every app: its stylesheet sits just above that file's priority.

### How settings are written

- **dmsh**: `~/.config/dmsh/config.kdl` is edited in place — only the values you change; comments,
  layout and KDL version stay as they are. Each save is checked with `dmsh config check` first,
  and dmsh applies it live (feature switches need a restart, which the page offers).
- **niri**: dm-manager writes only its own file, `~/.config/niri/dm-manager.kdl`. The first
  time, the Keyboard & Input page asks to add one line to the end of your `config.kdl`:
  `include optional=true "dm-manager.kdl"`. Being last, its values win; your own files are never
  edited, and the page shows what's in effect across all of them. Each save is checked with
  `niri validate`, and niri reloads it live.
- **Cursor**: besides niri, the theme and size go to GTK (gsettings, `settings.ini`) and
  Qt/KDE (`kcminputrc`), so apps match.
- **Default apps**: through GIO, into `~/.config/mimeapps.list` (KDE's app cache is refreshed so
  Dolphin agrees). The terminal goes to `xdg-terminals.list` and kdeglobals' `TerminalApplication`;
  niri shortcuts that start the old terminal can be pointed at the new one (in `dm-manager.kdl`).
- **Qt/KDE look**: kdeglobals through KDE's tools (`plasma-apply-colorscheme`, `kwriteconfig6`),
  qt6ct and Kvantum's config; running KDE apps are told to reload. If niri sets
  `QT_STYLE_OVERRIDE`, it's updated too (in `dm-manager.kdl`).
- **GTK look**: gsettings (which GTK 4 apps follow live) and the `settings.ini` files. A `gtk.css`
  left by a theming tool is switched off by renaming it, so it can be switched back on.
- **Idle**: `~/.config/swayidle/config`, edited in place; a timer switched off is commented out
  (`# dm-manager: off: …`) so switching it on restores your command. swayidle is restarted to apply.

## Install

```bash
git clone https://github.com/Kexfff/dm-manager && cd dm-manager
./install.sh
```

This checks for the build requirements, builds a release binary and installs it to
`~/.local/bin`, with a launcher entry and the icon, so DM Manager shows up in your app launcher.
`./install.sh --prefix DIR` installs elsewhere; `./install.sh --uninstall` removes it all.

Requirements: Rust, GTK 4, libadwaita 1.9 or newer, and the services the pages talk to:
NetworkManager for Wi-Fi and BlueZ (`bluetooth.service`) for Bluetooth. A page whose service is
missing says so instead of failing.

## Layout

```
crates/
  dmm-sys/      backends, no GTK: network.rs (NetworkManager), bluetooth.rs (BlueZ and a
                pairing agent), power.rs (power-profiles-daemon, UPower), dmsh.rs, niri.rs,
                idle.rs (swayidle), appearance.rs and apps.rs (their configs), kdl_text.rs and
                ini.rs (in-place editing)
  dm-manager/   the app: app.rs (window, sidebar), pages/, widgets.rs, theme.rs
    data/       style.css and bundled symbolic icons (compiled into a GResource)
dist/           launcher entry, and the app icon (icon.png, and the sizes made from it in icons/)
```

Each backend is an async loop: it takes requests from a channel and reports state snapshots
and events through a callback, so pages only handle plain data. It re-reads the state after
the service's D-Bus signals (debounced), so changes made elsewhere — `nmcli`, dmsh's Wi-Fi
toggle — show up immediately.

## Development

```bash
cargo run                                      # run from the tree
cargo test                                     # backend unit tests
cargo run -p dmm-sys --example probe           # print Wi-Fi and Bluetooth state, read-only
cargo run -p dmm-sys --example inputs          # print niri's input settings in effect, read-only
cargo run -p dmm-sys --example looks           # print the appearance settings in effect, read-only
DMM_PAGE=bluetooth DMM_SCREENSHOT=/tmp/bt.png cargo run   # render a page to PNG and quit
```

## License

GPL-3.0-or-later. The bundled symbolic icons (except the `dmm-shell`, `dmm-monitor` and
`dmm-assistant` ones) come from the [Adwaita icon theme](https://gitlab.gnome.org/GNOME/adwaita-icon-theme)
(LGPL-3.0 / CC-BY-SA-3.0).
