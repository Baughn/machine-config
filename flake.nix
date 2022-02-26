{
  description = "Machine configs";

  inputs.nixpkgs-stable.url = "flake:nixpkgs/nixos-21.11";
  inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable";
  #inputs.nixpkgs.url = "/home/svein/dev/nix/pkgs";
  #inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable-small";
  inputs.nixos-hardware.url = "flake:nixos-hardware";
  inputs.home-manager.url = "github:nix-community/home-manager";
  inputs.home-manager.inputs.nixpkgs.follows = "nixpkgs";
  inputs.deploy-rs.url = "github:serokell/deploy-rs";
  inputs.agenix.url = "github:ryantm/agenix";
  inputs.agenix.inputs.nixpkgs.follows = "nixpkgs";

  #inputs.openwrt = {
  #  url = "path:../openwrt";
  #  inputs.nixpkgs.follows = "nixpkgs";
  #};

  outputs = { self, nixpkgs, nixpkgs-stable, nixos-hardware, home-manager, deploy-rs, agenix }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      pkgs-stable = nixpkgs-stable.legacyPackages.${system};
      installer = modules: nixpkgs.lib.nixosSystem {
        inherit system modules;
      };
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
      deployNodes = hosts: pkgs.lib.listToAttrs (map (host: pkgs.lib.nameValuePair host
      {
        hostname = "${host}.brage.info";
        profiles.system = {
          user = "root";
          path = deploy-rs.lib.${system}.activate.nixos self.nixosConfigurations.${host};
        };
      }) hosts);
      node = { modules }: nixpkgs.lib.nixosSystem ({
        inherit system;
        modules = [{
          # Propagate nixpkgs
          nix.nixPath = [ "nixpkgs=/etc/nixpkgs" ];
          environment.etc."nixpkgs".source = nixpkgs;
        }
        # Add agenix
        agenix.nixosModules.age
        {
          environment.systemPackages = [ agenix.defaultPackage.${system} ];
        }
        ] ++ homeConfig ++ modules;
      });
    in {
      devShell.${system} = import ./shell.nix { inherit pkgs; };

      checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;

      packages.${system} = {
        install-cd = (installer [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ./installer/cd.nix
        ]).config.system.build.isoImage;
        install-kexec = (installer [
          "${nixpkgs}/nixos/modules/installer/netboot/netboot-minimal.nix"
          ./installer/kexec.nix
        ]).config.system.build.kexec_tarball;
      };

      deploy.nodes = deployNodes [ "tromso" "saya" "tsugumi" ];

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
          nixos-hardware.nixosModules.asus-battery {
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
