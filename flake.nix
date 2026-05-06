{
  inputs = {
    # Pinned past nixos-unstable to pick up PAM/apparmor fix (PR #511479).
    # Revert to `nixos-unstable` once the channel advances past 9d5a303.
    nixpkgs.url = "github:NixOS/nixpkgs/9d5a303cfbebf5931d29d75de01bbfecccf68a0e";
    nix-cachyos-kernel.url = "github:xddxdd/nix-cachyos-kernel/release";
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
    codex-cli-nix.url = "github:sadjow/codex-cli-nix";
    codex-cli-nix.inputs.nixpkgs.follows = "nixpkgs";
    ganbot.url = "git+file:/home/svein/dev/ganbot?ref=HEAD";
    ganbot.inputs.nixpkgs.follows = "nixpkgs";
    dessplay.url = "git+file:/home/svein/dev/dessplay?ref=HEAD";
    dessplay.inputs.nixpkgs.follows = "nixpkgs";
    agenix.url = "github:ryantm/agenix";
    agenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, nix-cachyos-kernel, home-manager, codex-cli-nix, ganbot, dessplay, agenix, ... }: {
    nixosConfigurations.saya = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit ganbot agenix; };
      modules = [
        home-manager.nixosModules.home-manager
        ./machines/saya
        ({ pkgs, ... }: {
          nixpkgs.overlays = [
            nix-cachyos-kernel.overlays.pinned
            (final: prev: {
              codex = codex-cli-nix.packages.${prev.stdenv.hostPlatform.system}.default;
              kdePackages = prev.kdePackages.overrideScope (kfinal: kprev: {
                kwin = kprev.kwin.overrideAttrs (old: {
                  # patches = (old.patches or []) ++ [ ./kwin.patch ];
                  src = ./kwin;
                });
              });
            })
          ];
        })
      ];
    };

    nixosConfigurations.v4 = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit agenix; };
      modules = [
        home-manager.nixosModules.home-manager
        ./machines/v4
      ];
    };

    nixosConfigurations.tsugumi = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit agenix dessplay; };
      modules = [
        home-manager.nixosModules.home-manager
        ./machines/tsugumi
      ];
    };
  };
}
