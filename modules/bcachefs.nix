{ lib, pkgs, ... }:

{
  boot.supportedFilesystems = ["bcachefs"];
  boot.initrd.supportedFilesystems = ["bcachefs"];

  # Makes `availableOn` fail for zfs, see <nixos/modules/profiles/base.nix>.
  # This is a workaround since we cannot remove the `"zfs"` string from `supportedFilesystems`.
  # The proper fix would be to make `supportedFilesystems` an attrset with true/false which we
  # could then `lib.mkForce false`
  nixpkgs.overlays = [(final: super: {
    zfs = super.zfs.overrideAttrs(_: {
      meta.platforms = [];
    });
  })];

  boot.kernelPatches = [ {
      name = "bcachefs-config";
      patch = null;
      extraConfig = ''
        PREEMPT_VOLUNTARY n
        PREEMPT y
        BCACHEFS_DEBUG y
        KALLSYMS y
        KALLSYMS_ALL y
        DEBUG_FS y
        DYNAMIC_FTRACE y
        FTRACE y
      '';
  } ];

  boot.kernelPackages = lib.mkForce pkgs.linuxPackages_latest;
}
