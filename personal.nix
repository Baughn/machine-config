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
      ./saya/default.nix
    ];
  };

  tsugumi = { config, pkgs, ... }: {
    deployment = {
      hasFastConnection = true;
      targetHost = "localhost";
    };

    imports = [
      ./tsugumi/default.nix
    ];
  };
  
  madoka = { config, pkgs, ... }: {
    deployment = {
      targetHost = "madoka.brage.info";
    };

    imports = [
      ./madoka/default.nix
    ];
  };

  homura = { config, pkgs, ... }: {
    deployment = {
      targetHost = "116.203.44.190";
    };

    imports = [ ./homura ];
  };

  tromso = { config, pkgs, ... }: {
    deployment = {
      targetHost = "tromso.brage.info";
    };

    imports = [
      ./tromso/default.nix
    ];
  };
}
