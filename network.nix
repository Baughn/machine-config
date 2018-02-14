{
  network = {
    description = "Personal machines (pets)";
    enableRollback = true;
  };
  
  defaults = {
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
      ./modules/desktop.nix
      ./modules/plex.nix
      ./modules/libvirtd.nix
      ./modules/nvidia.nix
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
      ./modules/plex.nix
    ];
  };
  
  madoka = { config, pkgs, ... }: {
    deployment = {
      targetHost = "madoka";
    };

    imports = [
      ./madoka/configuration.nix
#      ./modules/kubernetes-master.nix
    ];
  };

  # tromso = { config, pkgs, ... }: {
  #   deployment = {
  #     targetHost = "tromso.brage.info";
  #   };

  #   imports = [
  #     ./tromso/configuration.nix
  #   ];
  # };
}
