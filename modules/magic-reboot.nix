{ config, lib, pkgs, ... }:

let
  cfg = config.me.magicReboot;
  magicRebootModule = config.boot.kernelPackages.callPackage ../tools/magic-reboot/module/default.nix { };
  magicRebootSender = pkgs.callPackage ../tools/magic-reboot/sender/default.nix { };
in
{
  options.me.magicReboot = {
    enable = lib.mkEnableOption "magic packet emergency reboot";

    port = lib.mkOption {
      type = lib.types.port;
      default = 999;
      description = "UDP port to listen for magic packets";
    };

    dryrun = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "If true, log matches but don't actually reboot (for testing)";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.extraModulePackages = [ magicRebootModule ];
    boot.kernelModules = [ "magic_reboot" ];
    boot.extraModprobeConfig = ''
      options magic_reboot port=${toString cfg.port} key_path=${config.age.secrets."magic-reboot.key".path} dryrun=${if cfg.dryrun then "1" else "0"}
    '';

    # SysRq bitmask 128 enables the reboot command the kernel module triggers.
    boot.kernel.sysctl."kernel.sysrq" = lib.mkDefault 128;

    networking.firewall.allowedUDPPorts = [ cfg.port ];

    age.secrets."magic-reboot.key" = {
      file = ../secrets/magic-reboot.key.age;
      mode = "0440";
      owner = "root";
      group = "wheel";
    };

    environment.systemPackages = [ magicRebootSender ];
  };
}
