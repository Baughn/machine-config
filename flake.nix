{
  description = "Machine configurations for all my machines";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    nixpkgs-kernel.url = "github:nixos/nixpkgs?ref=nixos-unstable";

    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";

    colmena.url = "github:zhaofengli/colmena";
    colmena.inputs.nixpkgs.follows = "nixpkgs";

    agenix.url = "github:ryantm/agenix";
    agenix.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nixpkgs-kernel, nix-index-database, colmena, agenix, home-manager, ... }: {
    packages.x86_64-linux.options = (import (nixpkgs.outPath + "/nixos/release.nix") { }).options;

    # AIDEV-NOTE: VM tests for sanity checking configurations
    checks.x86_64-linux.basic-boot = import ./tests/basic-desktop.nix {
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
    };

    # Colmena deployment configuration
    colmenaHive = colmena.lib.makeHive {
      meta = {
        nixpkgs = import nixpkgs {
          system = "x86_64-linux";
          config.allowUnfree = true;
          overlays = [
            # Reuse existing overlay for zen kernel
            (final: prev: {
              inherit ((import nixpkgs-kernel {
                inherit (prev) system;
                config.allowUnfree = true;
              })) linuxPackages_zen;
            })
            # Add Colmena overlay
            colmena.overlays.default
          ];
        };
      };

      defaults = { name, nodes, ... }: {
        imports = [
          nix-index-database.nixosModules.nix-index
          agenix.nixosModules.default
          home-manager.nixosModules.home-manager
          ./secrets
        ];

        # Setup nix-index
        programs.nix-index-database.comma.enable = true;

        # Propagate nixpkgs
        nix.nixPath = [ "nixpkgs=/etc/nixpkgs" ];
        environment.etc."nixpkgs".source = nixpkgs;
        nix.registry.nixpkgs.flake = nixpkgs;

        # Common system packages
        environment.systemPackages = [
          colmena.packages.x86_64-linux.colmena
          agenix.packages.x86_64-linux.agenix
        ];

        # Bare-minimum home-manager setup
        home-manager.useGlobalPkgs = true;
        home-manager.useUserPackages = true;
        home-manager.users.svein = ./home/home.nix;
        # Automatically clobber pre-HM files
        home-manager.backupFileExtension = "backup";

        # Default deployment configuration
        deployment = {
          targetUser = "root";
          buildOnTarget = false; # Build locally
          replaceUnknownProfiles = true;
        };
      };

      # Deploy to current host (saya)
      saya = { name, nodes, ... }: {
        imports = [
          ./machines/saya/configuration.nix
        ];

        # Deployment configuration
        deployment = {
          targetHost = "localhost"; # Deploy to local machine
          allowLocalDeployment = true;
        };
      };

      # v4 proxy server
      v4 = { name, nodes, ... }: {
        imports = [
          ./machines/v4/configuration.nix
        ];

        # Deployment configuration
        deployment = {
          targetHost = "v4.brage.info";
          tags = [ "remote" ];
        };
      };

      # tsugumi server
      tsugumi = { name, nodes, ... }: {
        imports = [
          ./machines/tsugumi/configuration.nix
        ];

        # Deployment configuration
        deployment = {
          targetHost = "tsugumi.local";
          tags = [ "remote" ];
        };
      };
    };
  };
}

