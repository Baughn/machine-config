{ config, lib, options, pkgs, ... }:

let
  cfg = config.me.remoteBuilds;
in
{
  options.me.remoteBuilds = {
    enable = lib.mkEnableOption "remote Nix builders";

    builders = lib.mkOption {
      type = options.nix.buildMachines.type;
      default = [ ];
      description = "Non-local Nix build machines available to this host. The local host is added automatically.";
    };

    trustedUsers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "@wheel" ];
      description = "Users allowed to talk to the local Nix daemon as trusted users.";
    };
  };

  config = lib.mkIf cfg.enable {
    nix.distributedBuilds = true;
    nix.buildMachines = cfg.builders;

    nix.settings = {
      builders-use-substitutes = false;
      cores = lib.mkDefault 16;
      trusted-users = cfg.trustedUsers;
    };
  };
}
