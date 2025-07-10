{ pkgs, ... }:

{
  services.bazarr.enable = true;
  services.sonarr.enable = true;
  services.prowlarr.enable = true;
  services.radarr.enable = true;
  services.radarr.group = "sonarr";

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

