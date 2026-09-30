{ config, lib, pkgs, flakeSelf, ... }:

{
  options.me.deploy.rebootPatterns = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    example = [ "kwin" ];
    description = ''
      Store-path name regexes (beyond nix-deploy's built-in kernel, initrd,
      driver, and systemd checks) whose closure changes should make
      nix-deploy recommend `boot` instead of `switch` for this machine.
    '';
  };

  # What nix-deploy's lineage check reads back (`nixos-version --json`) to
  # refuse deploys that would roll back commits the machine is running.
  config.system.configurationRevision = flakeSelf.rev or flakeSelf.dirtyRev or null;

  # Baked into the toplevel so nix-deploy can read the policy from the path
  # it just built, without a second evaluation.
  config.system.systemBuilderCommands = ''
    cp ${
      pkgs.writeText "deploy-reboot-patterns.json"
        (builtins.toJSON config.me.deploy.rebootPatterns)
    } $out/deploy-reboot-patterns.json
  '';
}
