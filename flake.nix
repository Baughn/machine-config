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
  };

  outputs = { self, nixpkgs, nixpkgs-kernel, nix-index-database, colmena, agenix, ... }: {
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

      # Deploy to current host (saya)
      saya = { name, nodes, ... }: {
        imports = [
          ./machines/saya/configuration.nix
          nix-index-database.nixosModules.nix-index
          agenix.nixosModules.default
          ./secrets
        ];

        # Setup nix-index
        programs.nix-index-database.comma.enable = true;
        # Propagate nixpkgs
        nix.nixPath = [ "nixpkgs=/etc/nixpkgs" ];
        environment.etc."nixpkgs".source = nixpkgs;
        nix.registry.nixpkgs.flake = nixpkgs;

        environment.systemPackages = [
          colmena.packages.x86_64-linux.colmena
          agenix.packages.x86_64-linux.agenix
        ];

        # Deployment configuration
        deployment = {
          targetHost = "localhost"; # Deploy to local machine
          targetUser = "root";
          buildOnTarget = false; # Build locally
          allowLocalDeployment = true;
          replaceUnknownProfiles = true;
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
          targetUser = "root";
          buildOnTarget = false; # Build locally
          replaceUnknownProfiles = true;
	  tags = ["remote"];
        };
      };

      # tsugumi server
      tsugumi = { name, nodes, ... }: {
        imports = [
          ./machines/tsugumi/configuration.nix
          nix-index-database.nixosModules.nix-index
          agenix.nixosModules.default
          ./secrets
        ];

        # Setup nix-index
        programs.nix-index-database.comma.enable = true;
        # Propagate nixpkgs
        nix.nixPath = [ "nixpkgs=/etc/nixpkgs" ];
        environment.etc."nixpkgs".source = nixpkgs;
        nix.registry.nixpkgs.flake = nixpkgs;

        # Add Colmena to system packages
        environment.systemPackages = [ colmena.packages.x86_64-linux.colmena ];

        # Deployment configuration
        deployment = {
          targetHost = "tsugumi.local";
          targetUser = "root";
          buildOnTarget = false; # Build locally
          replaceUnknownProfiles = true;
	  tags = ["remote"];
        };
      };
    };
  };
}

