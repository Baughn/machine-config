{ lib, pkgs, ... }:

{
  # Useful flags: BCACHEFS_DEBUG y

  boot.kernelPatches = [{
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

  boot.kernelPackages =
    let
      kernel_pkg = { buildLinux, ... } @ args:
        buildLinux (args // rec {
          version = "6.7.3";
          modDirVersion = version;
          src = /home/svein/linux/patched;
          extraMeta.branch = "6.7";
          kernelPatches = [ ];
        } // (args.argsOverride or { }));
      kernel = pkgs.callPackage kernel_pkg { };
    in
    pkgs.recurseIntoAttrs (pkgs.linuxPackagesFor kernel);

  # Add delayacct to the kernel command line to actually turn it on.
  boot.kernelParams = [ "delayacct" ];
  # Unnecessary if you have a bcachefs filesystem in hardware-configuration.nix.
  boot.supportedFilesystems = [ "bcachefs" ];
  boot.initrd.supportedFilesystems = [ "bcachefs" ];
  # Use the latest kernel, since bcachefs is still in development.
  #boot.kernelPackages = lib.mkForce pkgs.linuxPackages_latest;
}
