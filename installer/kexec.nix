{ ... }:

{
  imports = [
    kexec/configuration.nix
  ];

  boot.supportedFilesystems = [ "zfs" ];
  networking.hostId = "12345678";

  kexec.autoReboot = false;

  users.users.root.openssh.authorizedKeys.keys = 
    (import ../modules/sshKeys.nix).svein;
}
