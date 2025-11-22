{
  description = "Machine configurations for all my machines";

  inputs = {
    # Default input
    #nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    nixpkgs.url = "git+file:///home/svein/dev/nixpkgs";
    # Default channel w/lag, sometimes used for individual currently broken packages
    nixpkgs-lagging.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    # Default channel without local changes (if any)
    nixpkgs-upstream.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    # Master branch (pre-build)
    nixpkgs-master.url = "github:nixos/nixpkgs?ref=master";

    determinate.url = "https://flakehub.com/f/DeterminateSystems/determinate/*";

    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";

    colmena.url = "github:zhaofengli/colmena";
    colmena.inputs.nixpkgs.follows = "nixpkgs";

    agenix.url = "github:ryantm/agenix";
    agenix.inputs.nixpkgs.follows = "nixpkgs";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    lanzaboote.url = "github:nix-community/lanzaboote";
    lanzaboote.inputs.nixpkgs.follows = "nixpkgs";

    nix-darwin.url = "github:nix-darwin/nix-darwin/master";
    nix-darwin.inputs.nixpkgs.follows = "nixpkgs-upstream";

    ganbot.url = "git+file:///home/svein/dev/ganbot?ref=master";
    ganbot.inputs.nixpkgs.follows = "nixpkgs";

    background-process-manager.url = "github:Baughn/background-process-manager";
    background-process-manager.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nixpkgs-master, determinate, nix-index-database, colmena, agenix, home-manager, lanzaboote, nix-darwin, ... }@inputs:
    let
      # Custom library functions
      mylib = import ./lib { lib = nixpkgs.lib; };

      # Helper to extract just the options.json file from a derivation
      extractOptionsJson = system: optionsDrv: docPath:
        nixpkgs.legacyPackages.${system}.runCommand "options.json" { } ''
          cp ${optionsDrv}/${docPath} $out
        '';

      # Common modules for all NixOS systems
      commonModules = [
        determinate.nixosModules.default
        nix-index-database.nixosModules.nix-index
        agenix.nixosModules.default
        home-manager.nixosModules.home-manager
        ./secrets
        {
          # Setup nix-index
          programs.nix-index-database.comma.enable = true;

          # Propagate nixpkgs
          nix.nixPath = [ "nixpkgs=/etc/nixpkgs:/nix/var/nix/profiles/per-user/root/channels/nixos" ];
          environment.etc."nixpkgs".source = nixpkgs;
          nix.registry.nixpkgs.flake = nixpkgs;

          # Common system packages
          environment.systemPackages = [
            colmena.packages.x86_64-linux.colmena
            agenix.packages.x86_64-linux.agenix
          ];

          # Bare-minimum home-manager setup
          home-manager = {
            useGlobalPkgs = true;
            useUserPackages = true;
            users.svein = ./home/home.nix;
            # Automatically clobber pre-HM files
            backupFileExtension = "backup";
          };
        }
      ];

      # Helper function to create a NixOS configuration
      mkNixosConfiguration = { system ? "x86_64-linux", modules }: nixpkgs.lib.nixosSystem {
        inherit system;
        modules = commonModules ++ modules ++ [{
          nixpkgs.config.allowUnfree = true;
        }];
        specialArgs = { inherit inputs mylib; };
      };

      # Machine configurations
      machineConfigs = {
        saya = {
          modules = [ ./machines/saya/configuration.nix ];
          deployment = {
            targetHost = "localhost";
            allowLocalDeployment = true;
          };
        };
        testcase = {
          modules = [ ./machines/testcase/configuration.nix ];
          deployment = {
            targetHost = null;
            allowLocalDeployment = true;
          };
        };
        v4 = {
          modules = [ ./machines/v4/configuration.nix ];
          deployment = {
            targetHost = "v4.brage.info";
            tags = [ "remote" ];
          };
        };
        tsugumi = {
          modules = [ ./machines/tsugumi/configuration.nix ];
          deployment = {
            targetHost = "direct.brage.info";
            tags = [ "remote" ];
          };
        };
      };
    in
    {
      # Expose custom library functions
      lib = mylib;

      packages.x86_64-linux.options = extractOptionsJson "x86_64-linux"
        (import (nixpkgs.outPath + "/nixos/release.nix") { }).options
        "share/doc/nixos/options.json";
      packages.aarch64-darwin.options = extractOptionsJson "aarch64-darwin"
        nix-darwin.packages.aarch64-darwin.optionsJSON
        "share/doc/darwin/options.json";

      # Custom ISO image
      packages.x86_64-linux.iso = (mkNixosConfiguration {
        modules = [ ./machines/iso/configuration.nix ];
      }).config.system.build.isoImage;

      # Build all machine configurations
      packages.x86_64-linux.all-systems = nixpkgs.legacyPackages.x86_64-linux.linkFarm "all-systems"
        (builtins.map
          (name: {
            name = name;
            path = self.nixosConfigurations.${name}.config.system.build.toplevel;
          })
          (builtins.attrNames machineConfigs));

      # NixOS configurations for standard nixos-rebuild
      nixosConfigurations = builtins.mapAttrs
        (name: config:
          mkNixosConfiguration { modules = config.modules; }
        )
        machineConfigs;

      # Colmena deployment configuration
      colmenaHive = colmena.lib.makeHive ({
        meta = {
          nixpkgs = import nixpkgs {
            system = "x86_64-linux";
            config.allowUnfree = true;
            overlays = [
              # Add Colmena overlay
              colmena.overlays.default
            ];
          };
          specialArgs = { inherit inputs mylib; };
        };

        defaults = { name, nodes, ... }: {
          imports = commonModules;

          # Default deployment configuration
          deployment = {
            targetUser = "root";
            buildOnTarget = false; # Build locally
            replaceUnknownProfiles = true;
          };
        };
      } // (builtins.mapAttrs
        (name: config: { ... }: {
          imports = config.modules;
          deployment = config.deployment // {
            targetUser = "root";
            buildOnTarget = false;
            replaceUnknownProfiles = true;
          };
        })
        machineConfigs));

      # Darwin configuration for kaho
      darwinConfigurations."kaho" = nix-darwin.lib.darwinSystem {
        modules = [
          ./machines/kaho/configuration.nix
          home-manager.darwinModules.home-manager
          {
            # Home-manager configuration for macOS
            home-manager = {
              useGlobalPkgs = true;
              useUserPackages = true;
              users.svein = ./home/home.nix;
              # Automatically clobber pre-HM files
              backupFileExtension = "backup";
            };
          }
        ];
        specialArgs = { inherit inputs; };
      };
    };
}

