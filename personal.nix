let
  hercules-ci-agent =
    builtins.fetchTarball "https://github.com/hercules-ci/hercules-ci-agent/archive/stable.tar.gz";
in
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
      (hercules-ci-agent + "/module.nix")
    ];

    services.hercules-ci-agent = {
      enable = true;
      concurrentTasks = 4;
    };
    deployment.keys."cluster-join-token.key".keyFile = ./secrets/hercules-ci/cluster-join-token.key;
    deployment.keys."binary-caches.json".keyFile = ./secrets/hercules-ci/binary-caches.json;
  };
  
  madoka = { config, pkgs, ... }: {
    deployment = {
      targetHost = "madoka.brage.info";
    };

    imports = [
      ./madoka/default.nix
    ];
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
