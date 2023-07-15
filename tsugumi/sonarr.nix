{ pkgs, ... }:

{
  services.bazarr.enable = true;
  services.sonarr.enable = true;
  services.prowlarr.enable = true;
  services.jellyfin.enable = true;

  # Also run qbittorrent-nox as svein.
  systemd.services.qbittorrent-sonarr = {
    description = "qbittorrent";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    script = ''
      ${pkgs.qbittorrent-nox}/bin/qbittorrent-nox
    '';
    serviceConfig.User = "svein";
  };
}

