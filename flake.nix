{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nix-cachyos-kernel.url = "github:xddxdd/nix-cachyos-kernel/release";
    ganbot.url = "git+file:/home/svein/dev/ganbot?ref=HEAD";
    ganbot.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, nix-cachyos-kernel, ganbot, ... }: {
    nixosConfigurations.saya = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      specialArgs = { inherit ganbot; platform = "nixos"; };
      modules = [
        ./machines/saya
        ({ pkgs, ... }: {
          nixpkgs.overlays = [
            nix-cachyos-kernel.overlays.pinned
            (final: prev: {
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
  };
}
