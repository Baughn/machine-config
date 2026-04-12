{ pkgs, agenix, ... }:

{
  imports = [ agenix.nixosModules.default ];

  environment.systemPackages = [ agenix.packages.${pkgs.system}.default ];

  # Test secret — validates the pipeline; remove once real secrets are in place
  age.secrets.test.file = ../../secrets/test.age;
}
