{ config, pkgs, lib, ... }:

let
  cfg = config.services.zrepl;
in


{
  options = {
    services.zrepl = with lib; with types; {
      enable = mkEnableOption {
        name = "zrepl";
      };

      package = mkOption {
        default = pkgs.callPackage ../zrepl {};
        defaultText = "pkgs.zrepl";
        description = "zrepl package";
        type = package;
      };

      push = listOf (submodule {
        options = {
          rootFs = mkOption {
            description = "Root of ZFS dataset(s) to replicate";
            example = "rpool/home";
            type = string;
          };
          targetHost = mkOption {
            description = "DNS name or IP address of machine to replicate to. May be empty.";
            example = "example.org";
            type = string;
          };
          snapshotting = {
            prefix = mkOption {
              description = "Snapshot name prefix";
              default = "zrepl_";
              type = string;
            };
            interval = mkOption {
              description = "Time in minutes between snapshots";
              default = 10;
              type = int;
            };
          };
        };
      });
    };
  };

  config = {

    services.zrepl = {
      enable = true;
    };

    systemd.services.zrepl = lib.mkIf cfg.enable {
      enable = cfg.enable;

      description = "ZFS Replication";
      documentation = [ "https://zrepl.github.io/" ];

      requires = [ "local-fs.target" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        #User = "zrepl";
      };

      path = [ cfg.package pkgs.openssl ];
      script = ''
        set -e

        HOME=/var/spool/zrepl

        # Create the directories, if needed.
        if [[ ! -d $HOME ]]; then
          mkdir -m 0770 $HOME
          chown zrepl $HOME
          mkdir -m 0770 /var/run/zrepl
          chown zrepl /var/run/zrepl
        fi
        cd $HOME

        # Create target dataset
        if ! zfs list -H stash/zrepl; then
          zfs create stash/zrepl -o mountpoint=none
          # We do not intend to mount filesystems, and non-root users
          # anyway can't, but due to limitations in zfs-on-linux the user
          # still needs the permission.
          #
          # The mountpoint permission is needed because zrepl sets
          # mountpoint=none. TODO: Find some way to limit this.
          zfs allow -ldu zrepl mount,clone,create,destroy,hold,promote,receive,release,rename,rollback,snapshot,bookmark,userprop stash/zrepl
        fi

        # Initialize TLS certificates and copy to backup sinks.
      '';
    };

    users.users.zrepl = {
      uid = 316;
      isSystemUser = true;
      home = "/var/spool/zrepl";
    };

    environment.systemPackages = [
      (pkgs.callPackage ../zrepl {})
    ];
  };
}
