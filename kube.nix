{
  network = {
    description = "Kubernetes network";
    enableRollback = true;
  };
  
  defaults = { config, pkgs, ... }: {
    deployment.owners = [ "sveina@gmail.com" ];
    imports = [
      ./modules/basics.nix
      ./modules/emergency-shell.nix
    ];

    environment.etc = {
      nix-system-pkgs.source = /home/svein/dev/nix-stable;
      nixos.source = builtins.filterSource
        (path: type: baseNameOf path != "secrets" && type != "symlink" && !(pkgs.lib.hasSuffix ".qcow2" path))
        ./.;
    };
    nix.nixPath = [ "nixpkgs=/etc/nix-system-pkgs" ];
  };

  # km-01 = { configs, pkgs, ... }: {
  #   deployment = {
  #     targetHost = "195.201.27.41";
  #   };

  #   imports = [
  #     ./km-01/configuration.nix
  #     ./modules/kubernetes-master.nix
  #   ];
  # };
}
