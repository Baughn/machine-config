{system, pkgs}:

# This defines all the packages (i.e. software and init scripts) that get
# distributed. Packages can depend on each other, but this does not include
# runtime dependencies such as ports and hostnames; just the plain directory
# structures.

let
  callPackage = pkgs.lib.callPackageWith (pkgs // self);

  self = {
    ### Databases
    WikiDb = callPackage wiki/mediawiki-db.nix {};

    ### Web services
    Wiki = callPackage wiki/mediawiki.nix {};
  };
in self
