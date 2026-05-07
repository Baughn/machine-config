{ config, lib, pkgs, ... }:

let
  hardBlockedModules = lib.unique config.boot.blacklistedKernelModules;
in
{
  # `blacklist` only blocks automatic alias loading; `install` also makes
  # explicit `modprobe <module>` fail.
  #
  # /bin/false doesn't exist. But that's fine, the command just has to fail.
  boot.extraModprobeConfig = lib.mkIf (hardBlockedModules != [ ]) (
    lib.concatMapStringsSep "\n"
      (name: "install /bin/false")
      hardBlockedModules
  );
}
