let
  pkgs = (import /home/svein/dev/nix-system {}).pkgs;
  nixos = (import /home/svein/dev/nix-system/nixos);
in

rec {
  machines = [ "saya" "tsugumi" "madoka" "tromso" ];

  systems = pkgs.lib.genAttrs machines (machine: (nixos {
    configuration = builtins.toPath "/home/svein/nixos/${machine}/configuration.nix";
  }));

  all = pkgs.linkFarm "all-systems" (pkgs.lib.mapAttrsToList (machine: sys: {
    name = machine;
    path = pkgs.linkFarm machine [
      { name = "system";
        path = sys.system;
      }
      { name = "xfce-test";
        path = import /home/svein/dev/nix-system/nixos/tests/xfce.nix { config = sys.config; };
      }
      { name = "xmonad-test";
        path = import /home/svein/dev/nix-system/nixos/tests/xmonad.nix { config = sys.config; };
      }];
  }) systems);
}
