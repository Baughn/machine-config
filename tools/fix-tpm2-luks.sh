#!/usr/bin/env bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}TPM2 LUKS Re-enrollment Tool${NC}"
echo -e "${YELLOW}This tool re-enrolls LUKS devices with TPM2 after hardware changes${NC}\n"

# Check if running as root
if [[ $EUID -ne 0 ]]; then
   echo -e "${RED}This script must be run as root${NC}"
   exit 1
fi

# Find all LUKS devices
echo "Detecting LUKS devices..."
LUKS_DEVICES=$(lsblk -rno NAME,FSTYPE | grep crypto_LUKS | cut -d' ' -f1)

if [[ -z "$LUKS_DEVICES" ]]; then
    echo -e "${RED}No LUKS devices found${NC}"
    exit 1
fi

echo -e "Found LUKS devices:"
for dev in $LUKS_DEVICES; do
    echo "  - /dev/$dev"
done
echo

# Default PCRs (0=UEFI firmware, 7=Secure Boot state)
DEFAULT_PCRS="0+7"

echo "Available PCR configurations:"
echo "  0+7 - UEFI firmware + Secure Boot (default, most common)"
echo "  0+4+7 - UEFI + Boot loader + Secure Boot"
echo "  0+7+8 - UEFI + Secure Boot + Kernel cmdline"
echo
read -p "Enter PCRs to use (default: $DEFAULT_PCRS): " PCRS
PCRS=${PCRS:-$DEFAULT_PCRS}

echo
echo -e "${YELLOW}Will use PCRs: $PCRS${NC}"
echo

# Show current PCR values for reference
echo "Current PCR values:"
systemd-analyze pcrs | grep -E "^    (0|4|7|8):" || true
echo

# Process each LUKS device
for dev in $LUKS_DEVICES; do
    DEVICE="/dev/$dev"
    echo -e "\n${GREEN}Processing $DEVICE${NC}"
    
    # Check if device is already unlocked by finding its mapper device
    MAPPER_NAME=$(lsblk -rno NAME "$DEVICE" | grep -v "^${dev}$" | head -1 || true)
    
    if [[ -n "$MAPPER_NAME" && -e "/dev/mapper/$MAPPER_NAME" ]]; then
        echo "  Device is unlocked (mapper: $MAPPER_NAME)"
    else
        echo -e "${YELLOW}  Device appears to be locked${NC}"
        read -p "  Do you want to unlock it now? (y/N): " unlock
        if [[ "$unlock" =~ ^[Yy]$ ]]; then
            cryptsetup open "$DEVICE" "crypt-${dev##*/}"
        else
            echo "  Skipping locked device"
            continue
        fi
    fi
    
    # Check for existing TPM2 enrollment
    echo "  Checking for existing TPM2 enrollment..."
    if systemd-cryptenroll "$DEVICE" | grep -q tpm2; then
        echo "  Found existing TPM2 enrollment, removing..."
        systemd-cryptenroll --wipe-slot=tpm2 "$DEVICE" || {
            echo -e "${RED}  Failed to remove TPM2 enrollment${NC}"
            continue
        }
        echo "  Old TPM2 enrollment removed"
    else
        echo "  No existing TPM2 enrollment found"
    fi
    
    # Re-enroll with TPM2
    echo "  Enrolling with TPM2 (PCRs: $PCRS)..."
    read -p "  Do you want to enroll this device? (y/N): " confirm
    if [[ "$confirm" =~ ^[Yy]$ ]]; then
        if systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs="$PCRS" "$DEVICE"; then
            echo -e "${GREEN}  Successfully enrolled $DEVICE with TPM2${NC}"
        else
            echo -e "${RED}  Failed to enroll $DEVICE with TPM2${NC}"
        fi
    else
        echo "  Skipped"
    fi
done

echo
echo -e "${GREEN}TPM2 re-enrollment complete!${NC}"
echo
echo "Note: You may need to regenerate your initramfs/initrd and reboot"
echo "      for the changes to take effect on boot."
echo
echo "On NixOS, this will happen automatically on next rebuild."