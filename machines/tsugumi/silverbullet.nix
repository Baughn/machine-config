{ config
, lib
, pkgs
, ...
}: {
  services.silverbullet = {
    enable = true;
    listenAddress = "127.0.0.1";
    # Caddy authenticates requests; local-web-access.nix prevents other local
    # accounts from reaching this unauthenticated backend directly.
    openFirewall = false;
  };
}
