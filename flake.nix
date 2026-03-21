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
      specialArgs = { inherit ganbot; };
      modules = [
        ./configuration.nix
        ({ pkgs, ... }: {
          nixpkgs.overlays = [ nix-cachyos-kernel.overlays.pinned ];
        })
      ];
    };
  };
}
