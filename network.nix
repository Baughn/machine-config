{
  network = {
    description = "Personal machines (pets)";
    enableRollback = true;
  };
  
  defaults = { config, pkgs, ... }: {
    deployment.owners = [ "sveina@gmail.com" ];
    imports = [
      ./modules/basics.nix
      ./modules/emergency-shell.nix
    ];

    environment.etc = {
      nix-system-pkgs.source = /home/svein/dev/nix-system;
      nixos.source = builtins.filterSource
        (path: type: baseNameOf path != "secrets" && type != "symlink" && !(pkgs.lib.hasSuffix ".qcow2" path))
        ./.;
    };
    nix.nixPath = [ "nixpkgs=/etc/nix-system-pkgs" ];
  };

  saya = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "saya";  # Aka localhost
    };

    imports = [
      ./saya/configuration.nix
      ./modules/zfs.nix
      ./modules/desktop.nix
      ./modules/plex.nix
      ./modules/virtualisation.nix
      ./modules/nvidia.nix
      ./modules/rsyncd.nix
    ];
    
    systemd.enableEmergencyMode = true;
  };

  tsugumi = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "tsugumi";
    };

    imports = [
      ./tsugumi/configuration.nix
      ./modules/zfs.nix
      ./modules/plex.nix
    ];
  };
  
  madoka = { config, pkgs, ... }: {
    deployment = {
      targetHost = "madoka";
    };

    imports = [
      ./madoka/configuration.nix
      ./modules/zfs.nix
    ];
  };

  # tromso = { config, pkgs, ... }: {
  #   deployment = {
  #     targetHost = "tromso.brage.info";
  #   };

  #   imports = [
  #     ./tromso/configuration.nix
  #     ./modules/zfs.nix
  #   ];
  # };
}
