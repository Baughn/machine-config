{ lib, ... }:

{
  imports = [
    ../../modules/zsh.nix
  ];

  # Minimal required configuration
  networking.hostName = "testcase";

  # Required for evaluation but not for booting
  fileSystems."/" = {
    device = "/dev/null";
    fsType = "tmpfs";
  };

  boot.loader.grub.enable = false;

  system.stateVersion = "24.11";

  users.users.svein.group = "users";
  users.users.svein.isNormalUser = true;
}
