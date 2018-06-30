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

  tromso = { config, pkgs, ... }: {
    deployment = {
      targetHost = "tromso.brage.info";
    };

    imports = [
      ./tromso/configuration.nix
    ];
  };
}
