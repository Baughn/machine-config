{ config, lib, pkgs, inputs, ... }:

{
  imports = [
    inputs.lanzaboote.nixosModules.lanzaboote
  ];

  # Disable systemd-boot as lanzaboote replaces it
  boot.loader.systemd-boot.enable = lib.mkForce false;

  # Enable lanzaboote
  boot.lanzaboote = {
    enable = true;
    pkiBundle = "/var/lib/sbctl";
  };

  # Enable systemd in initrd (required for TPM support if needed later)
  boot.initrd.systemd.enable = true;

  # Install sbctl for key management
  environment.systemPackages = with pkgs; [
    sbctl
  ];
}
