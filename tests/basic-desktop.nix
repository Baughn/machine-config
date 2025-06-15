# AIDEV-NOTE: Basic VM test to verify NixOS configuration builds and boots
{ pkgs, ... }:

pkgs.nixosTest {
  name = "basic-desktop";

  nodes.machine = { config, pkgs, ... }: {
    # Import a minimal subset of the configuration
    imports = [
      ../modules
    ];

    # Basic system configuration
    boot.loader.grub.enable = false;
    fileSystems."/" = {
      device = "tmpfs";
      fsType = "tmpfs";
    };

    # Minimal services for testing
    services.getty.autologinUser = "root";
  };

  testScript = ''
    machine.wait_for_unit("multi-user.target")
    machine.succeed("systemctl is-active multi-user.target")
    print("Basic boot test passed!")
  '';
}
