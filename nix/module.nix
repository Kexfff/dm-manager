# NixOS module: `programs.dm-manager.enable = true;`
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.dm-manager;
in
{
  options.programs.dm-manager = {
    enable = lib.mkEnableOption "DM Manager, settings and tools for the DM-Shell desktop";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "dm-manager.packages.\${system}.default";
      description = "The DM Manager package to use.";
    };

    withKdeTools = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Include KDE's tools (kwriteconfig6, plasma-apply-colorscheme) for theming Qt apps
        such as Dolphin. Turn off to keep Plasma's libraries out of the closure.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ (cfg.package.override { inherit (cfg) withKdeTools; }) ];

    # GTK settings (theme, icons, cursor, fonts) are stored in dconf.
    programs.dconf.enable = lib.mkDefault true;
    # Battery status on the Power & Sleep page.
    services.upower.enable = lib.mkDefault true;
  };
}
