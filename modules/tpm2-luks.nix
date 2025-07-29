{ config, lib, pkgs, ... }:

# TPM2 LUKS Unlock Configuration
# ==============================
#
# This module enables TPM2-based automatic unlocking of LUKS2 encrypted devices.
# It works alongside lanzaboote secure boot to provide a secure, passwordless boot experience.
#
# MANUAL SETUP REQUIRED:
# After applying this configuration, you must manually enroll each LUKS device:
#
# 1. List available TPM2 devices (verify TPM is available):
#    sudo systemd-cryptenroll --tpm2-device=list
#
# 2. Enroll each LUKS device with TPM2:
#    sudo systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=0+2+7+12 /dev/nvme0n1p2
#    sudo systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=0+2+7+12 /dev/nvme1n1p1
#    sudo systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=0+2+7+12 /dev/nvme2n1p1
#
#    You'll be prompted for the LUKS passphrase for each device.
#
# 3. Verify enrollment (check for systemd-tpm2 token):
#    sudo cryptsetup luksDump /dev/nvme0n1p2 | grep -A20 "Tokens:"
#
# PCR Selection:
# - PCR 0: Core system firmware code (UEFI firmware measurements)
# - PCR 2: Option ROM code (additional firmware components)
# - PCR 7: Secure Boot state and keys (used by lanzaboote)
# - PCR 12: Kernel command line and initrd measurements
#
# The TPM will only release the LUKS keys if these PCRs match the enrolled state,
# ensuring the system hasn't been tampered with.
#
# To remove TPM2 enrollment (if needed):
# sudo systemd-cryptenroll --wipe-slot=tpm2 /dev/nvme0n1p2

{
  # Enable TPM2 support
  security.tpm2 = {
    enable = true;
    # Enable udev rules for TPM device access
    applyUdevRules = true;
    # Enable TCTI environment for TPM2 tools
    tctiEnvironment = {
      enable = true;
      interface = "device";
    };
  };

  # Add TPM kernel modules to initrd for early boot unlock
  boot.initrd.availableKernelModules = [ "tpm_tis" "tpm_crb" ];

  # Ensure we have the necessary packages for TPM2 operations
  environment.systemPackages = with pkgs; [
    tpm2-tss
    tpm2-tools
  ];

  # systemd in initrd is already enabled in secure-boot.nix
  # Just ensure we have the right setup for TPM2 unlock
  boot.initrd.systemd = {
    # Enable TPM2 support in initrd
    extraBin = {
      # systemd-cryptenroll needs these binaries in initrd
      "systemd-cryptenroll" = "${pkgs.systemd}/bin/systemd-cryptenroll";
    };
  };
}
