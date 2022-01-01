{ config, pkgs, lib, ...}:

{
  boot.initrd.availableKernelModules = [
    "igb"  # Intel Gigabit (saya, tsugumi, tromso)
  ];
  # Run SSHD even in emergency mode.
  systemd.services.sshd.wantedBy = [ "emergency.target" "rescue.target" ];
  systemd.services.dhcpcd.wantedBy = [ "emergency.target" "rescue.target" ];

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
