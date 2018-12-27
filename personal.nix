rec {
  network = {
    description = "Personal machines (pets)";
    enableRollback = true;
  };
  
  defaults = { config, pkgs, ... }: {
    deployment.owners = [ "sveina@gmail.com" ];
  };

  saya = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "10.40.0.3";
    };

    imports = [
      ./saya/configuration.nix
    ];
  };

  tsugumi = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "localhost";
    };

    imports = [
      ./tsugumi/configuration.nix
    ];
  };
  
  madoka = { config, pkgs, ... }: {
    deployment = {
      targetHost = "madoka.brage.info";
      targetEnv = "hetzner";
      hetzner = {
        inherit (import secrets/hetzner.nix) robotUser robotPass;
        mainIPv4 = "95.216.71.247";
        partitions = ''
          clearpart --all --initlabel --drives=nvme0n1,nvme1n1

          part swap1 --recommended --label=swap1 --fstype=swap --ondisk=nvme0n1
          part swap2 --recommended --label=swap2 --fstype=swap --ondisk=nvme1n1

          part btrfs.1 --ondisk=nvme0n1 --size=16000
          part btrfs.2 --ondisk=nvme1n1 --size=16000

          btrfs / --data=1 --metadata=1 --label=root btrfs.1 btrfs.2
        '';
      };
    };

    imports = [
      modules/default.nix
      madoka/configuration.nix
    ];
  };

  tromso = { config, pkgs, ... }: {
    deployment = {
      targetHost = "tromso.brage.info";
    };

    imports = [
      ./tromso/configuration.nix
    ];
  };
}
