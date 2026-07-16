{ config, lib, pkgs, ... }:

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

  # Baked into the toplevel so nix-deploy can read the policy from the path
  # it just built, without a second evaluation.
  config.system.systemBuilderCommands = ''
    cp ${
      pkgs.writeText "deploy-reboot-patterns.json"
        (builtins.toJSON config.me.deploy.rebootPatterns)
    } $out/deploy-reboot-patterns.json
  '';
}
