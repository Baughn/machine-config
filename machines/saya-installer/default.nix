{ config, pkgs, lib, modulesPath, agenix, sshKeys, flakeSelf, nix-cachyos-kernel, ... }:

let
  encryptedHostKey = ../../secrets/saya-host-key.enc;

  repoSrc = lib.cleanSourceWith {
    name = "nixos-flake-source";
    src = ../..;
    filter = path: type:
      let base = baseNameOf (toString path); in
      !(base == ".git"
        || base == "target"
        || base == "result"
        || lib.hasPrefix "result-" base
        || base == ".direnv");
  };
in
{
  imports = [
    "${modulesPath}/installer/netboot/netboot-base.nix"
    ../../modules/cli-tools.nix
    agenix.nixosModules.default
  ];

  nixpkgs.overlays = [ nix-cachyos-kernel.overlays.pinned ];

  # Same kernel family as saya (zen4 cachy) but without saya's aggressive
  # hardware-strip overrides — rescue images want broad driver coverage.
  boot.kernelPackages = lib.mkForce pkgs.cachyosKernels.linuxPackages-cachyos-latest-zen4;

  networking.hostName = "saya-installer";
  networking.hostId = "deadbeef";

  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = lib.mkForce "no";
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
    };
    hostKeys = lib.mkForce [
      { path = "/etc/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
    ];
  };

  users.users.nixos.openssh.authorizedKeys.keys = sshKeys.svein;

  boot.supportedFilesystems.zfs = true;
  boot.supportedFilesystems.btrfs = true;
  boot.zfs.package = config.boot.kernelPackages.zfs_cachyos;
  boot.zfs.forceImportRoot = false;

  system.activationScripts.decryptHostKey = {
    deps = [ "specialfs" ];
    text = ''
      set -eu
      mkdir -p /etc/ssh
      if [ ! -s /etc/ssh/ssh_host_ed25519_key ]; then
        # --console writes/reads directly on /dev/console. Without it,
        # systemd-ask-password queues a request to /run/systemd/ask-password/
        # and waits for an agent — but during NixOS activation no agent
        # is running yet, so the prompt is invisible and the script hangs.
        passphrase=$(${pkgs.systemd}/bin/systemd-ask-password \
          --timeout=0 \
          --console \
          "Saya host key passphrase: ")

        # iter must match the value used to create secrets/saya-host-key.enc.
        # 20M iterations ≈ 8–10 s of PBKDF2 on saya (Zen 4).
        printf '%s' "$passphrase" | ${pkgs.openssl}/bin/openssl enc -d \
          -aes-256-cbc -pbkdf2 -iter 20000000 \
          -in ${encryptedHostKey} \
          -out /etc/ssh/ssh_host_ed25519_key \
          -pass stdin
        chmod 600 /etc/ssh/ssh_host_ed25519_key

        ${pkgs.openssh}/bin/ssh-keygen -y -f /etc/ssh/ssh_host_ed25519_key \
          > /etc/ssh/ssh_host_ed25519_key.pub
        chmod 644 /etc/ssh/ssh_host_ed25519_key.pub
        unset passphrase
      fi
    '';
  };

  system.activationScripts.agenixNewGeneration.deps =
    lib.mkAfter [ "decryptHostKey" ];

  age.secrets = {
    wireguard-saya = { file = ../../secrets/wireguard-saya.age; };
    restic-password = { file = ../../secrets/restic-password.age; };
    magic-reboot = { file = ../../secrets/magic-reboot.key.age; };
    redis-nixcheck-password = { file = ../../secrets/redis-nixcheck-password.age; };
  };

  systemd.services.populate-nixos-repo = {
    description = "Seed /home/nixos/nixos with the flake working tree";
    wantedBy = [ "multi-user.target" ];
    after = [ "local-fs.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if [ ! -e /home/nixos/nixos ]; then
        ${pkgs.coreutils}/bin/cp -r ${repoSrc} /home/nixos/nixos
        ${pkgs.coreutils}/bin/chown -R nixos:users /home/nixos/nixos
        ${pkgs.findutils}/bin/find /home/nixos/nixos -type d -exec chmod u+rwx {} +
        ${pkgs.findutils}/bin/find /home/nixos/nixos -type f -exec chmod u+rw {} +
      fi
    '';
  };

  system.stateVersion = lib.mkDefault lib.trivial.release;
}
