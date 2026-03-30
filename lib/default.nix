{ lib }:

{
  # Given a module directory and platform string ("nixos" or "darwin"), returns
  # the platform-appropriate file. The wrong platform's file is never evaluated —
  # this is critical because even referencing a nonexistent option (behind mkIf)
  # is a compile error in Nix.
  mkPlatformModule = platform: dir:
    let
      file = dir + "/${platform}.nix";
    in
      if builtins.pathExists file
      then import file
      else throw "Module ${toString dir} has no ${platform}.nix";
}
