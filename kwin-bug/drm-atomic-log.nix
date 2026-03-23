# NixOS module that injects the drm-atomic-log shim into display manager
# compositor processes for debugging DRM atomic modeset calls.
#
# Usage: add to your imports, then set drm-atomic-log.enable = true.
# Works with both SDDM and GDM without enabling either.

{ config, lib, pkgs, ... }:

let
  cfg = config.drm-atomic-log;
  pkg = pkgs.callPackage ./default.nix {};
  logDir = "/tmp/drm-atomic-log";
in
{
  options.drm-atomic-log = {
    enable = lib.mkEnableOption "DRM atomic modeset ioctl logging";
  };

  config = lib.mkIf cfg.enable {
    # Ensure the log directory exists with correct permissions
    systemd.tmpfiles.rules = [
      "d ${logDir} 1777 root root -"
    ];

    # SDDM: override CompositorCommand to wrap kwin_wayland with env
    services.displayManager.sddm.settings = lib.mkIf config.services.displayManager.sddm.enable {
      Wayland = {
        CompositorCommand = lib.concatStringsSep " " [
          "env"
          "LD_PRELOAD=${pkg}/lib/drm-atomic-log.so"
          "DRM_SHIM_LOG_DIR=${logDir}"
          "${pkgs.kdePackages.kwin}/bin/kwin_wayland"
          "--no-global-shortcuts"
          "--no-kactivities"
          "--no-lockscreen"
          "--locale1"
        ];
      };
    };

    # GDM: inject via PAM environment for the greeter session
    security.pam.services.gdm-launch-environment.text =
      lib.mkIf config.services.displayManager.gdm.enable (lib.mkAfter ''
        session required pam_env.so conffile=${
          pkgs.writeText "drm-shim-env" ''
            LD_PRELOAD DEFAULT=${pkg}/lib/drm-atomic-log.so
            DRM_SHIM_LOG_DIR DEFAULT=${logDir}
          ''
        } readenv=0
      '');
  };
}
