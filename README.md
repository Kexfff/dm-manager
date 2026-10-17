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
| Shell — dmsh's config (colors, font, island sizes, clock, quick controls) | planned |
| Keyboard & Input — layouts, cursor theme and size, touchpad | planned |
| Default Apps | planned |
| Appearance — Qt/GTK themes, Dolphin included | planned |
| Power & Sleep — dim, lock and suspend timers | planned |
| Wallpapers — browse and download from Wallhaven | planned |
| Assistant, System Monitor | later |

Pick the accent from the colored dot at the top of the sidebar. It's saved in
`~/.config/dm-manager/settings.kdl`.

dm-manager keeps its own look even when a theming tool writes colors into
`~/.config/gtk-4.0/gtk.css` for every app: its stylesheet sits just above that file's priority.

Settings that belong to niri will be written to their own file, `~/.config/niri/dm-manager.kdl`,
included last from your `config.kdl`; dm-manager won't edit the niri files you write by hand.

## Install

```bash
git clone https://github.com/Kexfff/dm-manager && cd dm-manager
./install.sh
```

This builds a release binary and installs it to `~/.local/bin`, with a desktop entry and an
icon. `./install.sh --uninstall` removes them.

Requirements: Rust, GTK 4, libadwaita 1.9 or newer, and the services the pages talk to:
NetworkManager for Wi-Fi and BlueZ (`bluetooth.service`) for Bluetooth. A page whose service is
missing says so instead of failing.

## Layout

```
crates/
  dmm-sys/      system backends over D-Bus (no GTK): network.rs (NetworkManager),
                bluetooth.rs (BlueZ, including the pairing agent)
  dm-manager/   the app: app.rs (window, sidebar), pages/, widgets.rs, theme.rs
    data/       style.css and bundled symbolic icons (compiled into a GResource)
dist/           desktop entry and app icon
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
DMM_PAGE=bluetooth DMM_SCREENSHOT=/tmp/bt.png cargo run   # render a page to PNG and quit
```

## License

GPL-3.0-or-later. The bundled symbolic icons (except the `dmm-shell`, `dmm-monitor` and
`dmm-assistant` ones) come from the [Adwaita icon theme](https://gitlab.gnome.org/GNOME/adwaita-icon-theme)
(LGPL-3.0 / CC-BY-SA-3.0).
