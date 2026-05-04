{ config, lib, options, ... }:

let
  cfg = config.me.remoteBuilds;
in
{
  options.me.remoteBuilds = {
    enable = lib.mkEnableOption "remote Nix builders";

    builders = lib.mkOption {
      type = options.nix.buildMachines.type;
      default = [ ];
      description = "Remote Nix build machines available to this host.";
    };

    trustedUsers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "@wheel" ];
      description = "Users allowed to talk to the local Nix daemon as trusted users.";
    };
  };

  config = lib.mkIf cfg.enable {
    nix.distributedBuilds = cfg.builders != [ ];
    nix.buildMachines = cfg.builders;

    nix.settings = {
      builders-use-substitutes = true;
      trusted-users = cfg.trustedUsers;
    };
  };
}
