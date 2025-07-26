{ config, lib, pkgs, ... }:

{
  # Enable NFS server
  services.nfs.server = {
    enable = true;
    exports = ''
      # Export home directory only to saya via WireGuard (10.171.0.6)
      /home/svein 10.171.0.6(rw)
    '';
  };

  # Enable rpcbind for NFS
  services.rpcbind.enable = true;

  # Firewall configuration - only allow NFS on WireGuard interface
  networking.firewall = {
    interfaces.wg0 = {
      allowedTCPPorts = [
        111 # rpcbind
        2049 # NFS
        20048 # mountd
      ];
      allowedUDPPorts = [
        111 # rpcbind
        2049 # NFS
        20048 # mountd
      ];
    };
  };
}
