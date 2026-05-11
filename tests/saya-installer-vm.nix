{ pkgs, lib, installerCfg }:

let
  installerEntry = "saya-installer.conf";
  installerKernel = "${installerCfg.system.build.kernel}/${installerCfg.system.boot.loader.kernelFile}";
  installerInitrd = "${installerCfg.system.build.netbootRamdisk}/initrd";
in
pkgs.testers.nixosTest {
  name = "saya-installer-vm";

  nodes.machine =
    { pkgs, config, lib, ... }:
    let
      diskImage = import "${pkgs.path}/nixos/lib/make-disk-image.nix" {
        inherit config lib pkgs;
        label = "nixos";
        format = "qcow2";
        partitionTableType = "efi";
        touchEFIVars = true;
        installBootLoader = true;
        copyChannel = false;

        # The installer netboot initrd embeds a squashfs store and is much
        # larger than the qemu-vm module's default 256 MiB ESP.
        bootSize = "4G";
      };
    in
    {
      virtualisation = {
        memorySize = 12288;
        directBoot.enable = false;
        useBootLoader = lib.mkForce false;
        useDefaultFilesystems = false;
        useEFIBoot = true;
        efi.variables = "${diskImage}/efi-vars.fd";
        diskImage = null;
        fileSystems = {
          "/" = {
            device = "/dev/vda2";
            fsType = "ext4";
          };
          "/boot" = {
            device = "/dev/vda1";
            fsType = "vfat";
            noCheck = true;
          };
        };
        qemu.drives = [
          {
            name = "installer-boot";
            file = "${diskImage}/nixos.qcow2";
            driveExtraOpts = {
              format = "qcow2";
              snapshot = "on";
            };
            deviceExtraOpts = {
              bootindex = "1";
              serial = "saya-installer-vm";
            };
          }
        ];
      };

      system.build.diskImage = diskImage;
      system.switch.enable = true;

      boot.loader = {
        timeout = 0;
        efi.canTouchEfiVariables = true;
        systemd-boot = {
          enable = true;
          editor = false;
          extraFiles = {
            "efi/saya-installer/kernel" = installerKernel;
            "efi/saya-installer/initrd" = installerInitrd;
          };
          extraEntries.${installerEntry} = ''
            title  NixOS Installer (saya, VM smoke test)
            linux  /efi/saya-installer/kernel
            initrd /efi/saya-installer/initrd
            options init=${installerCfg.system.build.toplevel}/init ${toString installerCfg.boot.kernelParams} console=ttyS0
            sort-key z_installer
          '';
          extraInstallCommands = ''
            ${lib.getExe pkgs.gnused} -i \
              's|^default .*|default ${installerEntry}|' \
              ${config.boot.loader.efi.efiSysMountPoint}/loader/loader.conf
          '';
        };
      };
    };

  testScript = ''
    import time

    machine.start()

    deadline = time.time() + 300
    while time.time() < deadline:
        console = machine.get_console_log()

        if "saya-installer login:" in console:
            break

        if "Failed to load initrd" in console or "Out of resources" in console:
            raise Exception("saya-installer failed in EFI/systemd-boot before Linux could start")

        time.sleep(1)
    else:
        raise Exception("timed out waiting for saya-installer login prompt")
  '';
}
