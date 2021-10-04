{ config, pkgs, lib, ...}:

{
  boot.initrd.availableKernelModules = [
    "igb"  # Intel Gigabit (saya, tsugumi)
    "e1000e"  # Intel Gigabit (madoka)
  ];
  systemd.enableEmergencyMode = lib.mkDefault false;
  boot.initrd.network = {
    # Not until the device naming is fixed!
    enable = false;
    ssh = {
      enable = true;
      authorizedKeys = (import ./sshKeys.nix).svein;
      # Use a fixed host key, but--
#      hostRSAKey = (import ../secrets).hostRSAKey.${config.deployment.targetHost};
      # Don't run on the standard port, so nixops won't get confused.
      port = 2222;
    };
  };
}
