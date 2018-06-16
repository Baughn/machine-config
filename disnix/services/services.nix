{system, pkgs, distribution, invDistribution}:

# This defines running applications, with runtime dependencies. Nothing
# complicated for just mediawiki.

let
  custom = import ./build.nix { inherit system pkgs; };
in

rec {
  ### Databases
  WikiDb = {
    name = "WikiDb";
    pkg = custom.WikiDb;
    dependsOn = {};
    type = "mysql-database";
  };

  ### Web applications
  Wiki = {
    name = "Wiki";
    pkg = custom.Wiki;
    dependsOn = {
      inherit WikiDb;
    };
    type = "apache-webapplication";
  };
}
