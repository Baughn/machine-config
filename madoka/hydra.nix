{ config, pkgs, lib, ... }:

{
  services.hydra = {
    debugServer = true;
    enable = true;
    hydraURL = "https://hydra.brage.info/";
    minimumDiskFree = 20;
    notificationSender = "hydra@brage.info";
    smtpHost = "localhost";
    port = 3001;
  };
  services.postgresql = {
    enable = true;
  };
  nix.buildMachines = [{
    sshUser = "builder";
    hostName = "madoka.brage.info";
    maxJobs = 2;
    system = "x86_64-linux";
    sshKey = "/etc/nixos/keys/builder";
    supportedFeatures = [ ];
  }];
  users.extraUsers.builder = {
    isNormalUser = true;
    uid = 1018;
    extraGroups = [ ];
    openssh.authorizedKeys.keys = [ (builtins.readFile "/etc/nixos/keys/builder.pub") ];
  };
  programs.ssh.knownHosts = [{
    hostNames = [ "madoka.brage.info" "localhost" ];
    publicKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFE7yNm04ZPysVoYsu0ZkC85eeRu63qsgKuOIkopmm/y";
  }];
  nix.trustedUsers = [ "root" "builder" ];
}
