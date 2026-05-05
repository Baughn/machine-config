{ pkgs, agenix, ... }:

{
  imports = [ agenix.nixosModules.default ];

  environment.systemPackages = [ agenix.packages.${pkgs.stdenv.hostPlatform.system}.default ];
}
