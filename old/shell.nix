{ pkgs }:
pkgs.buildEnv {
  name = "nixos-config-env";
  paths = with pkgs; [ git-crypt ];
}
