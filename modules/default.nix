{ ... }:

{
  imports = [
    ./agenix.nix
    ./cli-tools.nix
    ./dns.nix
    ./firejail.nix
    ./home-manager.nix
    ./kernel-modules.nix
    ./magic-reboot.nix
    ./mdns.nix
    ./nix.nix
    ./nix-build-balancer.nix
    ./remote-builds.nix
    ./security.nix
    ./shell.nix
    ./ssh-auth.nix
    ./ssh.nix
    ./wireguard.nix
    ./zfs.nix
  ];
}
