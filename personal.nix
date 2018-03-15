rec {
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
  };

  saya = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "saya";  # Aka localhost
    };

    imports = [
      ./saya/configuration.nix
    ];
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
