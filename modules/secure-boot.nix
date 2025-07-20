{ config, lib, pkgs, inputs, ... }:

{
  imports = [
    inputs.lanzaboote.nixosModules.lanzaboote
  ];

  # Boot configuration
  boot = {
    # Disable systemd-boot as lanzaboote replaces it
    loader.systemd-boot.enable = lib.mkForce false;

    # Enable lanzaboote
    lanzaboote = {
      enable = true;
      pkiBundle = "/var/lib/sbctl";
    };

    # Enable systemd in initrd (required for TPM support if needed later)
    initrd.systemd.enable = true;
  };

  # Install sbctl for key management
  environment.systemPackages = with pkgs; [
    sbctl
  ];
}
