{
  description = "Machine configs";

  inputs.nixpkgs-stable.url = "flake:nixpkgs/nixos-23.05";
  inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable";

  inputs.nixos-hardware.url = "flake:nixos-hardware";

  inputs.home-manager.url = "github:nix-community/home-manager";
  inputs.home-manager.inputs.nixpkgs.follows = "nixpkgs";

  inputs.deploy-rs.url = "github:serokell/deploy-rs";
  inputs.deploy-rs.inputs.nixpkgs.follows = "nixpkgs";

  inputs.agenix.url = "github:ryantm/agenix";
  inputs.agenix.inputs.nixpkgs.follows = "nixpkgs";

  inputs.vscode.url = "github:nix-community/nixos-vscode-server";
  inputs.vscode.inputs.nixpkgs.follows = "nixpkgs";

  inputs.nix-index-database.url = "github:nix-community/nix-index-database";
  inputs.nix-index-database.inputs.nixpkgs.follows = "nixpkgs";

  inputs.flox.url = "github:flox/flox";

  #inputs.lix = {
  #  url = "git+https://git.lix.systems/lix-project/lix?ref=refs/tags/2.90-beta.1";
  #  flake = false;
  #};
  #inputs.lix-module = {
  #  url = "git+https://git.lix.systems/lix-project/nixos-module";
  #  inputs.lix.follows = "lix";
  #  inputs.nixpkgs.follows = "nixpkgs";
  #};

  # Flake outputs:
  # - One machine config for each of my machines.
  # - Packages:
  #  - install-kexec, which is a custom installer that loads through kexec.
  #  - install-cd, which is a customized version of the regular installer.
  # - deploy and checks, which are used by deploy-rs. Which I don't use anymore. Yeah.
  # - devShell, aka. ./shell.nix.
  #
  # Each machine config also includes the home-manager config for my normal user.
  #
  outputs = {
    self,
    nixpkgs,
    nixpkgs-stable,
    nixos-hardware,
    home-manager,
    deploy-rs,
    agenix,
    vscode,
    nix-index-database,
    flox,
    #lix,
    #lix-module,
  }: let
    system = "x86_64-linux";
    stateVersion = "23.05";
    pkgs = nixpkgs.legacyPackages.${system};
    pkgs-stable = nixpkgs-stable.legacyPackages.${system};
    installer = modules:
      nixpkgs.lib.nixosSystem {
        inherit system modules;
      };
    # Imported by each machine config.
    homeConfig = [
      home-manager.nixosModules.home-manager
      {
        home-manager = {
          useGlobalPkgs = true;
          useUserPackages = true;
          users.svein = import ./home/home.nix;
        };
      }
    ];
    deployNodes = hosts:
      pkgs.lib.listToAttrs (map (host:
        pkgs.lib.nameValuePair host
        {
          hostname = "${host}.brage.info";
          profiles.system = {
            user = "root";
            path = deploy-rs.lib.${system}.activate.nixos self.nixosConfigurations.${host};
          };
        })
      hosts);
    node = {modules}:
      nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          flox = flox.packages.${system};
        };
        modules =
          [
            #lix-module.nixosModules.default
            {
              system.stateVersion = stateVersion;
              # Propagate nixpkgs
              nix.nixPath = ["nixpkgs=/etc/nixpkgs"];
              environment.etc."nixpkgs".source = nixpkgs;
              nix.registry.nixpkgs.flake = nixpkgs;
              programs.nix-index-database.comma.enable = true;
            }
            nix-index-database.nixosModules.nix-index
            # Add vscode for vscode-server.
            #vscode.nixosModules.default
            #{
            #  services.vscode-server.enable = true;
            #  services.vscode-server.enableFHS = true;
            #  nixpkgs.config.permittedInsecurePackages = [
            #  ];
            #}
            # Add agenix for secret management.
            agenix.nixosModules.age
            {
              # Agenix
              environment.systemPackages = [agenix.packages.${system}.default];
              age.identityPaths = [
                "/etc/ssh/ssh_host_ed25519_key"
                "/home/svein/.ssh/id_ed25519"
              ];
            }
          ]
          ++ homeConfig
          ++ modules;
      };
  in {
    formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.alejandra;

    devShell.${system} = import ./shell.nix {inherit pkgs;};

    checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;

    packages.${system} = {
      install-cd =
        (installer [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ./installer/cd.nix
        ])
        .config
        .system
        .build
        .isoImage;
      install-kexec =
        (installer [
          "${nixpkgs}/nixos/modules/installer/netboot/netboot-minimal.nix"
          ./installer/kexec.nix
        ])
        .config
        .system
        .build
        .kexec_tarball;
    };

    deploy.nodes = deployNodes ["tromso" "saya" "tsugumi"];

    nixosConfigurations.saya = node {
      modules = [
        nixos-hardware.nixosModules.common-pc
        nixos-hardware.nixosModules.common-cpu-amd
        ./saya/configuration.nix
      ];
    };

    nixosConfigurations.kaho = node {
      modules = [
        nixos-hardware.nixosModules.asus-zephyrus-ga401
        nixos-hardware.nixosModules.asus-battery
        {
          hardware.asus.battery.chargeUpto = 70;
        }
        ./kaho/configuration.nix
      ];
    };

    nixosConfigurations.tsugumi = node {
      modules = [
        nixos-hardware.nixosModules.common-pc
        nixos-hardware.nixosModules.common-cpu-amd
        nixos-hardware.nixosModules.common-gpu-amd
        #openwrt.nixosModule
        ./tsugumi/configuration.nix
      ];
    };

    nixosConfigurations.tromso = node {
      modules = [
        nixos-hardware.nixosModules.common-pc
        nixos-hardware.nixosModules.common-cpu-amd
        nixos-hardware.nixosModules.common-gpu-amd
        ./tromso/configuration.nix
      ];
    };
  };
}
