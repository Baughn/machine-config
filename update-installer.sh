#! /usr/bin/env nix-shell
#! nix-shell -i bash -p jq shellcheck
# shellcheck shell=bash

set -euo pipefail

shellcheck "$0"

# Find Ventoy disk
device_info=$(lsblk -J | jq -r '.blockdevices[] | select(.children != null) | .children[] | select(.size == "465.7G")')

# Check if device_info is empty
if [ -z "$device_info" ]; then
  echo "Ventoy device not found."
  exit 1
fi

# Extract the device name and mount points
device_name=$(echo "$device_info" | jq -r '.name')
mount_points=$(echo "$device_info" | jq -r '.mountpoints[]')

# Check if the device is mounted
if [ "$mount_points" != "null" ]; then
  echo "Block device is already mounted at $mount_points"
  exit 1
else
  sudo mount "/dev/$device_name" /mnt
  trap 'sudo umount /mnt' EXIT
fi

# Build the NixOS installer
nix build '.#install-cd'

# Check if it's the same version already on the device.
# If so, don't bother updating.
iso=$(ls result/iso)
iso_hash=$(sha256sum "result/iso/$iso")
oldiso=$(cat /mnt/nixos-version.txt)
if [ "$iso_hash" = "$oldiso" ]; then
  echo "Already up to date."
  exit 0
fi

# Copy the new ISO to the device.
sudo cp "result/iso/$iso" /mnt/nixos-auto.iso
echo "$iso_hash" | sudo tee /mnt/nixos-version.txt > /dev/null
sync

# Done.
echo "Updated to $iso"
