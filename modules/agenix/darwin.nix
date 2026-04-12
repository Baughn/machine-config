{ pkgs, agenix, ... }:

{
  imports = [ agenix.darwinModules.default ];

  environment.systemPackages = [ agenix.packages.${pkgs.system}.default ];
}
