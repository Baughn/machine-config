# Settings for running non-nix software
{ config, pkgs, ... }:

let
  extraPkgs = pkgs: with pkgs; [
    bc
  ];
  extraLibraries = pkgs: with pkgs; [
    gperftools
  ];
in

{
  environment.systemPackages = with pkgs; [
      steamcmd
      (steam.override {
        inherit extraPkgs extraLibraries;
      }).run
  ] ++ (extraPkgs pkgs);

  programs.nix-ld.enable = true;
  programs.nix-ld.libraries = (extraLibraries pkgs);
}
