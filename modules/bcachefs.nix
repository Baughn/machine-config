{ lib, pkgs, ... }:

{
  # Useful flags: BCACHEFS_DEBUG y

  boot.kernelPatches = [ {
    name = "bcachefs-config";
    patch = null;
    extraConfig = ''
      PREEMPT_VOLUNTARY n
      PREEMPT y
      KALLSYMS y
      KALLSYMS_ALL y
      DEBUG_FS y
      DYNAMIC_FTRACE y
      FTRACE y
      TASK_DELAY_ACCT y
    '';
  }];

  boot.supportedFilesystems = ["bcachefs"];
  boot.initrd.supportedFilesystems = ["bcachefs"];
  boot.kernelPackages = lib.mkForce pkgs.linuxPackages_latest;
}
