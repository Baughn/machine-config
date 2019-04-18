{ config, pkgs, ... }:
{
  services.logrotate.enable = true;
  services.logrotate.config = ''
    compress
    compresscmd ${pkgs.zstd}/bin/zstd
    compressext zst

    /var/spool/nginx/logs/*.log {
      rotate 5
      weekly
      postrotate
        ${pkgs.psmisc}/bin/killall -USR1 nginx
      endscript
  '';
}
