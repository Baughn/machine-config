{
  description = "Machine configs";

  inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable";
  inputs.nixos-hardware.url = "flake:nixos-hardware";
  inputs.home-manager.url = "github:nix-community/home-manager";
  inputs.home-manager.inputs.nixpkgs.follows = "nixpkgs";
  inputs.deploy-rs.url = "github:serokell/deploy-rs";

  #inputs.openwrt = {
  #  url = "path:../openwrt";
  #  inputs.nixpkgs.follows = "nixpkgs";
  #};

  outputs = { self, nixpkgs, nixos-hardware, home-manager, deploy-rs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
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

      nixosConfigurations.saya = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          nixos-hardware.nixosModules.common-pc
          nixos-hardware.nixosModules.common-cpu-amd
    ./saya/configuration.nix
  ] ++ homeConfig;
      };

      nixosConfigurations.tsugumi = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          nixos-hardware.nixosModules.common-pc
          nixos-hardware.nixosModules.common-cpu-amd
          nixos-hardware.nixosModules.common-gpu-amd
          #openwrt.nixosModule
          ./tsugumi/configuration.nix
        ] ++ homeConfig;
      };

      nixosConfigurations.tromso = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          nixos-hardware.nixosModules.common-pc
          nixos-hardware.nixosModules.common-cpu-amd
          nixos-hardware.nixosModules.common-gpu-amd
          ./tromso/configuration.nix
        ] ++ homeConfig;
      };
    };
}
