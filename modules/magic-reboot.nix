{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.me.magicReboot;

  # Build kernel module for current kernel
  magicRebootModule = config.boot.kernelPackages.callPackage
    ../tools/magic-reboot/module/default.nix
    { };

  # Build sender tool
  magicRebootSender = pkgs.callPackage
    ../tools/magic-reboot/sender/default.nix
    { };
in
{
  options.me.magicReboot = {
    enable = mkEnableOption "magic packet emergency reboot";

    port = mkOption {
      type = types.port;
      default = 999;
      description = "UDP port to listen for magic packets";
    };

    dryrun = mkOption {
      type = types.bool;
      default = false;
      description = "If true, log matches but don't actually reboot (for testing)";
    };
  };

  config = mkIf cfg.enable {
    # Load the kernel module
    boot.extraModulePackages = [ magicRebootModule ];
    boot.kernelModules = [ "magic_reboot" ];

    # Module parameters via modprobe
    boot.extraModprobeConfig = ''
      options magic_reboot port=${toString cfg.port} key_path=${config.age.secrets."magic-reboot.key".path} dryrun=${if cfg.dryrun then "1" else "0"}
    '';

    # Enable SysRq reboot command (bitmask 128)
    boot.kernel.sysctl."kernel.sysrq" = mkDefault 128;

    # Open firewall port
    networking.firewall.allowedUDPPorts = [ cfg.port ];

    # Secret for the magic packet
    age.secrets."magic-reboot.key" = {
      file = ../secrets/magic-reboot.key.age;
      mode = "0440";
      owner = "root";
      group = "wheel"; # Allow svein (in wheel) to read for sender tool
    };
  };
}
