{config, pkgs, ...}:

# This file acts as an overlay on the machine configuration for Tsugumi.
# Not the cleanest way to do it, but eh.

{
  services.disnix.enable = true;
  services.mysql = {
    enable = true;
    package = pkgs.mysql;
    bind = "127.0.0.1";
    rootPassword = ../../secrets/mysql-pw;
    # We could define replication here.
    # If I had a machine to replicate the database too. :V
    #
    # Disnix also knows how to download snapshots of the database, which is fine
    # for the wiki but would be completely unreasonable for SV itself; need a
    # slave for that.
  };
  services.httpd = {
    enable = true;
    enablePHP = true;
    adminAddr = "webmaster@brage.info";
    hostName = "localhost";
    listen = [ { ip = "127.0.0.1"; port = 3300; } ];
    documentRoot = "/var/www";
    extraConfig = ''
      DirectoryIndex index.php
    '';
    # By default, Apache isn't built with access to git and diff. NixOS being
    # what it is, anything not explicitly included is inaccessible, so override
    # the package definition to add those.
    package = pkgs.apacheHttpd.overrideAttrs (oldAttrs: {
      nativeBuildInputs = (oldAttrs.nativeBuildInputs or []) ++ [ pkgs.makeWrapper ];
      postFixup = ''
        wrapProgram $out/bin/httpd --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git pkgs.diffutils ]}
      '';
    });
  };
}
