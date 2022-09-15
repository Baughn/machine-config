{pkgs, ...}: {
  imports = [
    kexec/configuration.nix
  ];

  boot.supportedFilesystems = ["zfs"];
  networking.hostId = "deafbeef";

  kexec.autoReboot = false;

  environment.systemPackages = with pkgs; [
    git
  ];

  users.users.root.openssh.authorizedKeys.keys =
    (import ../modules/sshKeys.nix).svein;
}
