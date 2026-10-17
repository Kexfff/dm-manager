#!/bin/sh
# Builds and installs DM Manager: the binary, a launcher entry and the icon, so it shows up
# in app launchers (walker, fuzzel, rofi…).
#
#   ./install.sh               install to ~/.local
#   ./install.sh --prefix DIR  install to DIR (bin/, share/applications/, share/icons/)
#   ./install.sh --uninstall   remove what was installed
#
# Needs Rust (cargo), GTK 4 and libadwaita 1.9+. Arch: pacman -S rust gtk4 libadwaita
set -eu

prefix="$HOME/.local"
uninstall=0
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) prefix="$2"; shift ;;
        --uninstall) uninstall=1 ;;
        -h | --help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

repo=$(cd "$(dirname "$0")" && pwd)
app_id=io.github.kexfff.DmManager
bin="$prefix/bin/dm-manager"
applications="$prefix/share/applications"
desktop="$applications/$app_id.desktop"
hicolor="$prefix/share/icons/hicolor"
sizes="16x16 24x24 32x32 48x48 64x64 128x128 256x256 512x512"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

refresh_caches() {
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$applications" || true
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t -f "$hicolor" 2>/dev/null || true
    # KDE apps (Dolphin's "Open with") read their own cache of launcher entries.
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
}

if [ "$uninstall" = 1 ]; then
    rm -f "$bin" "$desktop" "$hicolor/scalable/apps/$app_id.svg"
    for size in $sizes; do
        rm -f "$hicolor/$size/apps/$app_id.png"
    done
    refresh_caches
    say "Removed DM Manager from $prefix."
    exit 0
fi

# --- Requirements --------------------------------------------------------------------
command -v cargo >/dev/null || fail "cargo isn't installed (Arch: pacman -S rust, or rustup)"
if command -v pkg-config >/dev/null; then
    pkg-config --exists gtk4 || fail "GTK 4 isn't installed (Arch: pacman -S gtk4)"
    pkg-config --atleast-version=1.9 libadwaita-1 || fail "libadwaita 1.9 or newer is needed (Arch: pacman -S libadwaita)"
fi

# --- Build ---------------------------------------------------------------------------
say "Building (release)"
cargo build --release --locked --manifest-path "$repo/Cargo.toml"

# --- Install -------------------------------------------------------------------------
say "Installing to $prefix"
install -Dm755 "$repo/target/release/dm-manager" "$bin"
for size in $sizes; do
    install -Dm644 "$repo/dist/icons/$size/$app_id.png" "$hicolor/$size/apps/$app_id.png"
done
# An older version installed an SVG icon, which would win over the PNGs.
rm -f "$hicolor/scalable/apps/$app_id.svg"
# Launchers don't always have ~/.local/bin on PATH, so point at the binary directly.
mkdir -p "$applications"
sed "s|^Exec=.*|Exec=$bin|; s|^TryExec=.*|TryExec=$bin|" "$repo/dist/$app_id.desktop" >"$desktop"
chmod 644 "$desktop"
refresh_caches

say "Done. Open DM Manager from your launcher, or run: $bin"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) printf '    (%s isn'"'"'t on your PATH; the launcher entry works anyway.)\n' "$prefix/bin" ;;
esac
