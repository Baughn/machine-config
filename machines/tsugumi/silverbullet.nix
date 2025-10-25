{ config
, lib
, pkgs
, ...
}: {
  services.silverbullet = {
    enable = true;
    listenAddress = "127.0.0.1";
    # We use Caddy auth instead.
    openFirewall = false;
  };
}
