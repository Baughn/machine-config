{ config, lib, pkgs, ... }:

{
  imports = [
    ../../modules
    ./hardware-configuration.nix
    ./v4proxy.nix
  ];

  # Network — single dual-stack WAN interface via systemd-networkd.
  networking.hostName = "v4";
  networking.domain = "brage.info";
  networking.useNetworkd = true;

  systemd.network = {
    enable = true;
    networks."10-wan" = {
      matchConfig.Type = "ether";
      networkConfig.Address = [ "51.75.169.212/24" "2001:41d0:801:1000::22d7/64" ];
      networkConfig.Gateway = [ "51.75.169.1" "2001:41d0:801:1000::1" ];
    };
  };

  zramSwap.enable = true;

  time.timeZone = "Europe/Dublin";
  i18n.defaultLocale = "en_US.UTF-8";

  # Opt-in shared modules (default off).
  me.security.enable = true;
  me.magicReboot.enable = true;
  me.sshAuth.enable = true;

  users.users.svein = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = [
      "cert-authority ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGfsmAbJ1GKytVA71izC3xvIFYDQVHT2Q5CZPaIA6WqS svein@tsugumi"
    ];
  };

  # SSH-only landing user for ProxyJump into the IPv6 LAN.
  users.users.minecraft = {
    isNormalUser = true;
    uid = 1018;
    createHome = false;
    openssh.authorizedKeys.keys = [
      "cert-authority ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGfsmAbJ1GKytVA71izC3xvIFYDQVHT2Q5CZPaIA6WqS svein@tsugumi"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMWPdtIOeHF3TElJxL8gQGyZMErJHY0OdqrRFZFlFdP0 svein@svein-mac.roam.internal"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICeiqQnylmCVUzTNNcYRWKp/38dB5i3aGBs7ZB11MjkS svein@kaho.local"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPi0pCZqmvbObrDkAg28EBwt/hriKcCXRlEreexhoNJd bloxgate-ed25519-2018"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQChK01jTMsMS0sz7DOpArK6QyGd0vheV6gARoB5z6V/U5Y2Q6eXWnO1V4ZaE6HRQCBWp1KdwrqSSLUu9gS5kJTb1n+FsLx4wloCA1LggRSeQWinftZGxavhgdXm6/rnyDsnB42aHqusLiYaBL6OdlqFHiH916xzPxFktqd23rW64B1TV8VYcWa5ZxM8xd1n7svGfTXdb0O+w/RHIOLv4qKUmSmyJGPX7fGg+omHpPnTUpYV9CYInbWdy2nUoeiZU1TibkHuYaXaWLIzoWjIpuJqZdwqOkaK1e3WOrzkSYRHxteoNbzgcvQtdPAqM57TLM0Jwa3uJ5EiX6FCBaX0mZOZdZrNdQhQp0Yw3rkSKh7UySGimIBqFrJK9czVNbQBUUfllPL7x0zR8+PswYThpTecvfMoOoQoAVA2NQGSeglfeTQTgKxa/b85kdezMHoX+o/7hiItyez5tHmRbgfXxy2/8wco/w6hIhAujTeiUAMTu7xR0V8intUEXSuiI/arCJ0= lridge@kirin.local"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDLaHJT3oeYGloSHBzyy1ZX2EqGMcdF0/qneIbabQIBWXfS4hbYff63qrl58GxCaj5wlF9oCMf3OHxExI8ojH4LCKzQfocgW9j+ZbQC/UJjBSitFCcw9SqIHcOdaqLV9WnZOu1W2vcpAwwe6+nTmkmc+5uno0ChaWTh4SPLCfWWkSiTxeJlL1iF8adMGEeC8LpTyjZzjmh/GlG1lIwnInm98U9ipOdNRRR18jD13udu53xQTkY4LwqyYH49VsNXb4Rb673QyM8bwJ527keFZXQ430MKw8qj4NSnqpJDmOWUqqRLRczmOR473vRvUNT6u0cKMQI8McjdIAPoFevhEjUt/n56i1jFef1b9sMsNXYnpEkm9KM0SVnJ1im1jPkQQa6WneYwzxlkMh7V9XGnypB8d8W7IgD5X38NXzzHcDhCjIuL1huXFD8dleWVR78llz+QBmOGwb4er4fP/X+D0IvSz42FXGxmok9ulLgrFpVpCNQWIW9pcER2jrQMaKf/kTk= darqen@MAINCENTER"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQChri0A6naEZ/Dy7Hxxzr5rcQWzXr5YOHkhzjgPn04JhDnOgSEOYFvt6mZ/842bUQcvq5PpEwNxoliJt6mZ4R8SioNLLnNA/f9w4c1XzBbA/UQp+QNgaH8UmDdLL4Lurl13/uh8YGfND9iASL9eGXiGRXiFnvbvk9gPk2jhsiv5kB4lh8RvX7wngseNAAJc8/smicON95/JxcjAjYJwBqbd5dRvS7njIT0qgBik1rBWPLaX2NUb2Oefl5OQjQi7HDC6fgdcBS0ksKASRTm6uY3L3h3CXCOlVWhnLj0SCIsOqWmp8A9yKw7E7HBZ2IwkppZ2vp8Hx6vrEnobwTghbsc1 darqen27@freebsd"
      "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAhD/TX8mV0yCbuVsSE8Nno9YLBIMccUKmmEK4Ps1z77D+mi9F6HeaLFJ9FGIvhZ1BVChCJrNTNsTSqRcHLoDye3IsQa080mwhK2hRYPpKPhpa8/Y1zXP6bk6oePVeZDHR1tMgkRJzM/8kOpgNKRKIcdtFU7KWvCWywAeLi8BjrLE3fHU1a2I3ZrT4FUintgjtYVPD7p3m7AEsx4nvqOCxHqlt04i175bwvQ4HVgFzNYgQX9oQw4NgPfDsCCNXkDcVBwZNakUu8q9guHbjWO+1IXvUwfTUEk3pRpbM3glbWzba2PXJA+xM4/NYhZbfXqsXY8vMOmkC0Z54gVEN07s8lw== buizerd@localhost"
      "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAnGAL1GlHbpXT94UsbdGSk9tNoiRRQWuuYIg3KSJBRF0cj9BA5CIUXRFuiYyrxK2p4/tdzMG0vUhIPWELKtweVEfQpEvI6fKZNQ7jKuUBNRpWIw7PRZw4CAWehZjOwkTz1cF7hY+PcSawAAMINLRZG8g+KubiDf0pZLOqG+I2X1zXyEIH65rp3R1gKkN7+zfFjTT4kzXNksvP2wz1UI0msOaj+QtU1xqu2eaK4T9+wBU1X85uA3DlJx48REGrwzFKoo28WOY1neB08JLMhT1oD8W1U5kZZfJDAXPW0wiAxMUW4G2DSMFunvBQjtlXkgqShGVH7o1PP1AxO4jC5YPIaw== prestonfenette@gmail.com"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDjEbuzPf1C8vOVnCEVODIBQpUCpK7YaXpZwCphRGQRCCtYDot0NQPwc/Y7fUcxQc4ltQ8tu8nLbR9x5zk9KYfJ2xv4PfmXfnvVkwe9HrRtBfBuwiCsIbzBsmeJ0KvDCJDq+7KNU13LGmXGnPCMvMFX9EwE38z8uCaFUpIN/w/3sQoUepBCoPvFD7PBDfA2Tfk11v2+0KJhou2OJrBVK3y4ufxZIajfwvCc8E7EAtiPu5SQTlVcrP1JErsui+cRe/LEV16smDdwlzoSqR8SLd1tmW4Oo5rUwnBogiHVEzjhFBaR0Ql/vcBlfILBwikLwmP0OTUer9HMGS/DBs19exlKM+7d7nkrnf0MjV8S0UnhI3WAuSMybx7s6YxjZB+1LYd+9ZMNfS9l6YkhUgUUyhJ2dttKaa4IWEK5j7w1urcvAyT9A7UUJGJW+2UJ2z5yDVIRcWpLkW17dXmoTLcWpvqYqZtvZV5J8IoavGvVp/MMxvxd3S59trZUmQkBu9zF/W8="
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICGxbfR3P4jby0eDYiB1+/0uq1OvNQWXHTAET+YYlDNC"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIv3vZpMdKR4VCF9jkVigFhRUId+iRjoKcTXPmdkpbAE vindex@big"
      "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAjBmEEumO4pp05DRSaGTWkt22NZhcv1tJ+h9gKJvck8Em/WvdwG7l9qlXqu+URIRtosuh5S6M5imrEnTj/lkRC/RkxlH4MX172cNvgXJ9//Ge67wT3sZ/N9+CwfRIUgIMCdl14yJ3Q9+Vf3zlB64sp6pYmrDf7vClcDzzFgbbb3R2FRwRVjNqguSCP47Jb1XAgP0oQWc1JSYbUPLHwv7wIfarLfSsy/4KNi5z4ozT1JZUnpHfVygE5MUZyjB8EGZ7ZmpCa3duYxASTG8P9+dBk9iMDsI21XTektKsypk5qonlFdDoDZrFOAy9YQb7GMEcFvKjAYtG+GPDHxJcgOu6Pw== rsa-key-20161217"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOghAMI7V97vZudyc9fl4csb8VOILUhvKgt8ebk29cJU lucca@atropos"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDI61spDB1aui2nU4hmP0x6AH4LcQtm2qdpYysaC4FkY+6U3m8PyJNXPwRVDWS26++GPbB4EPG+1T4BgIdwCN2/3JTk6GwdCmIMh8bWlEdSs9B7cqtD+y5CwmTieWbgrU6fVZT9X6CRSZtz8KMkXlI/JgsI6qfJfNZVn7QPgp0FFhC6i72aNJGDvv4fneK5uYl9htM+NbyqFmO/YygYZhbwjTsYjhymW1aXO3A1+mckWmYR/fYRrtJ55ySso3hWTi6GzL6pQPEicEpWJM2J+/3xZ4kHCE9VHD2H97Vm9FkbBmyDkdXsDQ8WPzP2TA7ijvI29c3FHUsXSaU9PxP2H8ob lucca@kuu"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC34wZQFEOGkA5b0Z6maE3aKy/ix1MiK1D0Qmg4E9skAA57yKtWYzjA23r5OCF4Nhlj1CuYd6P1sEI/fMnxf+KkqqgW3ZoZ0+pQu4Bd8Ymi3OkkQX9kiq2coD3AFI6JytC6uBi6FaZQT5fG59DbXhxO5YpZlym8ps1obyCBX0hyKntD18RgHNaNM+jkQOhQ5OoxKsBEobxQOEdjIowl2QeEHb99n45sFr53NFqk3UCz0Y7ZMf1hSFQPuuEC/wExzBBJ1Wl7E1LlNA4p9O3qJUSadGZS4e5nSLqMnbQWv2icQS/7J8IwY0M8r1MsL8mdnlXHUofPlG1r4mtovQ2myzOx clever@nixos"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMaaKLw4S7s6RjvFoeN+fRNmmaUeSEvdqWzL/bUy5SaF jared@arch01d.ctha.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBpUHboM5fNLogflF/9EEklCgAvmE08L1lmT696UIwSW jared@arch01l.ctha.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG5nvmj3DlRnRJqsvdizUOMcQH71XQxew5jI6WW65Gpv jared@win01d.ctha.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXD8KKr1XyV3aOsb9eeagSrLY3A5L1nPgXnLO6XpSwc maxwell.lt@maxwell-nixos"
    ];
  };

  system.stateVersion = "23.11";
}
