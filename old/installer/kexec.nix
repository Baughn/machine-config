{pkgs, ...}: {
  imports = [
    kexec/configuration.nix
    ../modules/zfs.nix
  ];

  boot.supportedFilesystems = ["zfs"];
  networking.hostId = "deafbeef";

  kexec.autoReboot = false;

  environment.systemPackages = with pkgs; [
    git
  ];

  users.users.root.openssh.authorizedKeys.keys =
    (import ../modules/keys.nix).svein.ssh;
}
