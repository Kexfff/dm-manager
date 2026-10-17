#!/bin/sh
# Builds and installs dm-manager: the binary, its desktop entry and icon.
#
#   ./install.sh               install to ~/.local
#   ./install.sh --prefix DIR  install to DIR (bin/, share/applications/, share/icons/)
#   ./install.sh --uninstall   remove what was installed
#
# Needs Rust, GTK 4 and libadwaita 1.9+ (Arch: pacman -S gtk4 libadwaita).
set -eu

prefix="$HOME/.local"
uninstall=0
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) prefix="$2"; shift ;;
        --uninstall) uninstall=1 ;;
        -h | --help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

repo=$(cd "$(dirname "$0")" && pwd)
app_id=io.github.kexfff.DmManager
bin="$prefix/bin/dm-manager"
desktop="$prefix/share/applications/$app_id.desktop"
icon="$prefix/share/icons/hicolor/scalable/apps/$app_id.svg"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

if [ "$uninstall" = 1 ]; then
    rm -f "$bin" "$desktop" "$icon"
    say "Removed $bin, $desktop and $icon."
    exit 0
fi

say "Building (release)"
cargo build --release --manifest-path "$repo/Cargo.toml"

say "Installing to $prefix"
install -Dm755 "$repo/target/release/dm-manager" "$bin"
install -Dm644 "$repo/dist/$app_id.desktop" "$desktop"
install -Dm644 "$repo/dist/$app_id.svg" "$icon"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$prefix/share/applications" || true
gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true

say "Done. Run dm-manager, or find DM Manager in your launcher."
