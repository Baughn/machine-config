{ pkgs, ... }:

{
  services = {
    bazarr.enable = true;
    sonarr = {
      enable = true;
      user = "svein";
      group = "sonarr";
    };
    prowlarr.enable = true;
    radarr = {
      enable = true;
      group = "sonarr";
    };
  };

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
