{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook4,
  glib,
  gtk4,
  libadwaita,
  gdk-pixbuf,
  librsvg,
  gsettings-desktop-schemas,
  xkeyboard_config,
  # Tools the pages run, found after anything on the user's PATH.
  coreutils,
  procps,
  util-linux,
  pciutils,
  dbus,
  systemd,
  kdePackages,
  # KDE's tools for theming Qt apps (kwriteconfig6, plasma-apply-colorscheme, kbuildsycoca6).
  # Without them, the Qt parts of Appearance fall back to editing kdeglobals directly.
  withKdeTools ? true,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  runtimeTools = [
    coreutils
    procps
    util-linux
    pciutils
    dbus
    systemd
    glib.bin
  ]
  ++ lib.optionals withKdeTools [
    kdePackages.kconfig
    kdePackages.kservice
    kdePackages.plasma-workspace
  ];
in
rustPlatform.buildRustPackage {
  pname = "dm-manager";
  inherit (cargoToml.workspace.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../dist
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [
    "--package"
    "dm-manager"
  ];
  cargoTestFlags = [ "--workspace" ];

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook4
    # glib-compile-resources, for the bundled stylesheet and icons
    glib
  ];

  buildInputs = [
    gtk4
    libadwaita
    glib
    gdk-pixbuf
    # SVG icons and previews
    librsvg
    gsettings-desktop-schemas
  ];

  postInstall = ''
    install -Dm644 dist/io.github.kexfff.DmManager.desktop -t $out/share/applications
    for size in 16 24 32 48 64 128 256 512; do
      install -Dm644 dist/icons/''${size}x''${size}/io.github.kexfff.DmManager.png \
        $out/share/icons/hicolor/''${size}x''${size}/apps/io.github.kexfff.DmManager.png
    done
  '';

  preFixup = ''
    gappsWrapperArgs+=(
      --suffix PATH : ${lib.makeBinPath runtimeTools}
      --set-default XKB_CONFIG_ROOT ${xkeyboard_config}/share/X11/xkb
    )
  '';

  meta = {
    description = "Settings and tools for the DM-Shell desktop on niri";
    homepage = "https://github.com/Kexfff/dm-manager";
    license = lib.licenses.gpl3Plus;
    mainProgram = "dm-manager";
    platforms = lib.platforms.linux;
  };
}
