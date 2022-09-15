{
  config,
  pkgs,
  lib,
  ...
}: {
  boot.initrd.availableKernelModules = [
    "igb" # Intel Gigabit (saya, tsugumi, tromso)
  ];
  # Run SSHD even in emergency mode.
  systemd.services.sshd.wantedBy = ["emergency.target" "rescue.target"];
  systemd.services.dhcpcd.wantedBy = ["emergency.target" "rescue.target"];

  # Allow login during initrd, in case it hangs.
  boot.initrd.network = {
    enable = true;
    ssh = {
      enable = true;
      authorizedKeys = (import ./sshKeys.nix).svein;
      # Use a fixed host key. The same one as for the main host, thanks.
      hostKeys = ["/etc/ssh/ssh_host_ed25519_key"];
    };
  };

  # Systemd has a default 10 minute reboot watchdog, but it requires a watchdog device.
  boot.kernelModules = ["softdog"];
}
