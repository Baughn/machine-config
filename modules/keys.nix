# This file contains SSH and Wireguard keys for various users.
# When adding yourself, make sure your username matches the one in users.nix.
#
# There's no need for the allowedIPs field to list your exact IP address,
# as it's intended primarily to blacklist the 95% of the internet that's
# essentially the Warp. Feel free to include your entire ISP. If you don't
# know what address that should be, then google for "what is my IP" and
# run the resulting address through whois; the CIDR block is what you want.
#
# The id field is really just the LSB of your IP, and
# must be a unique number >1.

{
  svein = {
    ssh = [
      "cert-authority ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGfsmAbJ1GKytVA71izC3xvIFYDQVHT2Q5CZPaIA6WqS svein@tsugumi"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMWPdtIOeHF3TElJxL8gQGyZMErJHY0OdqrRFZFlFdP0 svein@svein-mac.roam.internal"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICeiqQnylmCVUzTNNcYRWKp/38dB5i3aGBs7ZB11MjkS svein@kaho.local"
    ];
    wireguard = [{
      publicKey = "9u3/F1o4ImItDXJCMr06YpEuUKCqX9cuQdG0dlTdQCE=";
      allowedIPs = [ "89.101.222.210/29" ];
      id = 6;
    }];
  };
  bloxgate = {
    ssh = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPi0pCZqmvbObrDkAg28EBwt/hriKcCXRlEreexhoNJd bloxgate-ed25519-2018" ];
    wireguard = [{
      publicKey = "6OMjCrzgoBe3iAnXGlhcce/za/poemekSpE95BuCmXc=";
      allowedIPs = [
        "2600:1700:eec7:820e::/64"
        "172.11.128.0/22"
      ];
      id = 2;
    }];
  };
  dusk = {
    ssh = [
      "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAnGAL1GlHbpXT94UsbdGSk9tNoiRRQWuuYIg3KSJBRF0cj9BA5CIUXRFuiYyrxK2p4/tdzMG0vUhIPWELKtweVEfQpEvI6fKZNQ7jKuUBNRpWIw7PRZw4CAWehZjOwkTz1cF7hY+PcSawAAMINLRZG8g+KubiDf0pZLOqG+I2X1zXyEIH65rp3R1gKkN7+zfFjTT4kzXNksvP2wz1UI0msOaj+QtU1xqu2eaK4T9+wBU1X85uA3DlJx48REGrwzFKoo28WOY1neB08JLMhT1oD8W1U5kZZfJDAXPW0wiAxMUW4G2DSMFunvBQjtlXkgqShGVH7o1PP1AxO4jC5YPIaw== prestonfenette@gmail.com"
    ];
  };
  darqen27 = {
    ssh = [
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDLaHJT3oeYGloSHBzyy1ZX2EqGMcdF0/qneIbabQIBWXfS4hbYff63qrl58GxCaj5wlF9oCMf3OHxExI8ojH4LCKzQfocgW9j+ZbQC/UJjBSitFCcw9SqIHcOdaqLV9WnZOu1W2vcpAwwe6+nTmkmc+5uno0ChaWTh4SPLCfWWkSiTxeJlL1iF8adMGEeC8LpTyjZzjmh/GlG1lIwnInm98U9ipOdNRRR18jD13udu53xQTkY4LwqyYH49VsNXb4Rb673QyM8bwJ527keFZXQ430MKw8qj4NSnqpJDmOWUqqRLRczmOR473vRvUNT6u0cKMQI8McjdIAPoFevhEjUt/n56i1jFef1b9sMsNXYnpEkm9KM0SVnJ1im1jPkQQa6WneYwzxlkMh7V9XGnypB8d8W7IgD5X38NXzzHcDhCjIuL1huXFD8dleWVR78llz+QBmOGwb4er4fP/X+D0IvSz42FXGxmok9ulLgrFpVpCNQWIW9pcER2jrQMaKf/kTk= darqen@MAINCENTER"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQChri0A6naEZ/Dy7Hxxzr5rcQWzXr5YOHkhzjgPn04JhDnOgSEOYFvt6mZ/842bUQcvq5PpEwNxoliJt6mZ4R8SioNLLnNA/f9w4c1XzBbA/UQp+QNgaH8UmDdLL4Lurl13/uh8YGfND9iASL9eGXiGRXiFnvbvk9gPk2jhsiv5kB4lh8RvX7wngseNAAJc8/smicON95/JxcjAjYJwBqbd5dRvS7njIT0qgBik1rBWPLaX2NUb2Oefl5OQjQi7HDC6fgdcBS0ksKASRTm6uY3L3h3CXCOlVWhnLj0SCIsOqWmp8A9yKw7E7HBZ2IwkppZ2vp8Hx6vrEnobwTghbsc1 darqen27@freebsd"
    ];
  };
  buizerd = {
    ssh = [ "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAhD/TX8mV0yCbuVsSE8Nno9YLBIMccUKmmEK4Ps1z77D+mi9F6HeaLFJ9FGIvhZ1BVChCJrNTNsTSqRcHLoDye3IsQa080mwhK2hRYPpKPhpa8/Y1zXP6bk6oePVeZDHR1tMgkRJzM/8kOpgNKRKIcdtFU7KWvCWywAeLi8BjrLE3fHU1a2I3ZrT4FUintgjtYVPD7p3m7AEsx4nvqOCxHqlt04i175bwvQ4HVgFzNYgQX9oQw4NgPfDsCCNXkDcVBwZNakUu8q9guHbjWO+1IXvUwfTUEk3pRpbM3glbWzba2PXJA+xM4/NYhZbfXqsXY8vMOmkC0Z54gVEN07s8lw== buizerd@localhost" ];
  };
  luke = {
    ssh = [
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQChK01jTMsMS0sz7DOpArK6QyGd0vheV6gARoB5z6V/U5Y2Q6eXWnO1V4ZaE6HRQCBWp1KdwrqSSLUu9gS5kJTb1n+FsLx4wloCA1LggRSeQWinftZGxavhgdXm6/rnyDsnB42aHqusLiYaBL6OdlqFHiH916xzPxFktqd23rW64B1TV8VYcWa5ZxM8xd1n7svGfTXdb0O+w/RHIOLv4qKUmSmyJGPX7fGg+omHpPnTUpYV9CYInbWdy2nUoeiZU1TibkHuYaXaWLIzoWjIpuJqZdwqOkaK1e3WOrzkSYRHxteoNbzgcvQtdPAqM57TLM0Jwa3uJ5EiX6FCBaX0mZOZdZrNdQhQp0Yw3rkSKh7UySGimIBqFrJK9czVNbQBUUfllPL7x0zR8+PswYThpTecvfMoOoQoAVA2NQGSeglfeTQTgKxa/b85kdezMHoX+o/7hiItyez5tHmRbgfXxy2/8wco/w6hIhAujTeiUAMTu7xR0V8intUEXSuiI/arCJ0= lridge@kirin.local"
    ];
    wireguard = [{
      publicKey = "jNO3umuWeSEYCtXUXVvT8RAl6nDm5gIYIMwYhSVDFTw=";
      allowedIPs = [
        "2603:6010:e400:a492::/64"
        "65.27.152.0/22"
      ];
      id = 7;
    }];
  };
  vindex = {
    ssh = [ "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCoFU4rmBlaVsQj/odpzWYtBu4j4TDQpNTTpkAIxQ+Qu/uikMkdq/CrSERgE+fPd6KLHz7nBCA5wIGoP0GDOCF/OShyP4F9KoQeCcuT1s2+rQBpRG3wjOftPil8V6Rw3dl5lLFcmyKDtNUpTQupenyH+nzlmBIJ9OPB5y7CIxulk+MgdA8dHF9IROGnfevJe4SkMdJixu4lgaGSx7JHztnq1RRN86FCIhtrc4qBJfbSwcrL3r3yGOK+trdXt2WAf1smXsskTmEAhpUnFrY5t74g9zfzmDJapnpxpUmAEY1N5Xg4xICAEOzma4Gi1CT8NRvUfsb/IkPUGfartASvRVbB" ];
  };
  einsig = {
    ssh = [ "ssh-rsa AAAAB3NzaC1yc2EAAAABJQAAAQEAjBmEEumO4pp05DRSaGTWkt22NZhcv1tJ+h9gKJvck8Em/WvdwG7l9qlXqu+URIRtosuh5S6M5imrEnTj/lkRC/RkxlH4MX172cNvgXJ9//Ge67wT3sZ/N9+CwfRIUgIMCdl14yJ3Q9+Vf3zlB64sp6pYmrDf7vClcDzzFgbbb3R2FRwRVjNqguSCP47Jb1XAgP0oQWc1JSYbUPLHwv7wIfarLfSsy/4KNi5z4ozT1JZUnpHfVygE5MUZyjB8EGZ7ZmpCa3duYxASTG8P9+dBk9iMDsI21XTektKsypk5qonlFdDoDZrFOAy9YQb7GMEcFvKjAYtG+GPDHxJcgOu6Pw== rsa-key-20161217" ];
  };
  lashtear = {
    ssh = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOghAMI7V97vZudyc9fl4csb8VOILUhvKgt8ebk29cJU lucca@atropos"
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDI61spDB1aui2nU4hmP0x6AH4LcQtm2qdpYysaC4FkY+6U3m8PyJNXPwRVDWS26++GPbB4EPG+1T4BgIdwCN2/3JTk6GwdCmIMh8bWlEdSs9B7cqtD+y5CwmTieWbgrU6fVZT9X6CRSZtz8KMkXlI/JgsI6qfJfNZVn7QPgp0FFhC6i72aNJGDvv4fneK5uYl9htM+NbyqFmO/YygYZhbwjTsYjhymW1aXO3A1+mckWmYR/fYRrtJ55ySso3hWTi6GzL6pQPEicEpWJM2J+/3xZ4kHCE9VHD2H97Vm9FkbBmyDkdXsDQ8WPzP2TA7ijvI29c3FHUsXSaU9PxP2H8ob lucca@kuu"
    ];
    wireguard = [{
      publicKey = "We6UVqoySg+bpp3tdVBpATZsdpuTH6/O1JeATcbfvVg=";
      allowedIPs = [
        "172.92.0.0/16"
        "45.56.94.73/32"
        "2604:4080::/32"
        "2600:3c01::f03c:91ff:fe96:8652/128"
      ];
      id = 4;
    }];
  };
  clever = {
    ssh = [
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC34wZQFEOGkA5b0Z6maE3aKy/ix1MiK1D0Qmg4E9skAA57yKtWYzjA23r5OCF4Nhlj1CuYd6P1sEI/fMnxf+KkqqgW3ZoZ0+pQu4Bd8Ymi3OkkQX9kiq2coD3AFI6JytC6uBi6FaZQT5fG59DbXhxO5YpZlym8ps1obyCBX0hyKntD18RgHNaNM+jkQOhQ5OoxKsBEobxQOEdjIowl2QeEHb99n45sFr53NFqk3UCz0Y7ZMf1hSFQPuuEC/wExzBBJ1Wl7E1LlNA4p9O3qJUSadGZS4e5nSLqMnbQWv2icQS/7J8IwY0M8r1MsL8mdnlXHUofPlG1r4mtovQ2myzOx clever@nixos"
    ];
  };
  jared = {
    # Will use jump.use1.ja4.org for traffic origination (IP address in Wireguard section)
    # SSH to this ec2 instance is limited to the various egress path(s) I have via an AWS security group,
    # or directly via ec2 instance connect, so it's pretty safe from the wild west of the internet.
    ssh = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILX9WJ0WLevr7KpBBQ0VTKMNYgMa8TA0puJXLmsgjtWi jared@jump.use1.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMaaKLw4S7s6RjvFoeN+fRNmmaUeSEvdqWzL/bUy5SaF jared@arch01d.ctha.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBpUHboM5fNLogflF/9EEklCgAvmE08L1lmT696UIwSW jared@arch01l.ctha.ja4.org"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG5nvmj3DlRnRJqsvdizUOMcQH71XQxew5jI6WW65Gpv jared@win01d.ctha.ja4.org"
    ];
    wireguard = [{
      # Tunnel from jump.use1.ja4.org
      publicKey = "3dw3YKuBXdSQj/ULDM9mj1VKotWNEWSNVW6FcIIsR2A=";
      allowedIPs = [ "35.168.203.255/32" ];
      id = 3;
    }];
  };
  maxwell-lt = {
    ssh = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXD8KKr1XyV3aOsb9eeagSrLY3A5L1nPgXnLO6XpSwc maxwell.lt@maxwell-nixos"
    ];
    wireguard = [{
      publicKey = "S+U8WhWiLl9NOzvFb1QGZg6brrGpnAVp0dfrQ5PsrCk=";
      allowedIPs = [ "75.46.0.0/16" ];
      id = 5;
    }];
  };
  # Machine-specific WireGuard entries
  tsugumi-machine = {
    ssh = [ ]; # No SSH needed for machine entries
    wireguard = [{
      publicKey = "y55YDIReEJ/lWrJiWYhxZ+grCPCJnqYlIN9LU7p6Yk0=";
      allowedIPs = [ ]; # Server accepts from anywhere in VPN
      id = 1;
    }];
  };
  saya-machine = {
    ssh = [ ]; # No SSH needed for machine entries
    wireguard = [{
      publicKey = "jyde6U7syzInvaQQfuIVUQKvYPJ51So51rFFDMkjKCE=";
      allowedIPs = [ ]; # Client connects to server
      id = 6;
    }];
  };
}
