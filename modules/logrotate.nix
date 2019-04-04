{ config, pkgs, ... }:
{
  services.logrotate.enable = true;
  services.logrotate.config = ''
    compress

    /var/spool/nginx/logs {
      rotate 5
      weekly
      size 100M
      postrotate
        ${pkgs.psmisc}/bin/killall -USR1 nginx
      endscript
  '';
}
