{ pkgs, ... }:

# eln.brage.info: the EA handbook (GuideME Markdown in Stasis main) as a static site, so
# admins can read it without logging in (Baughn, Discord 1574065275282792656; brief and
# renderer by LabAssistant, 1574067721782431764). A timer re-renders whenever Stasis main
# in the lab's shared git moves (a merge-queue landing); caddy-static serves the output
# (static-web.nix), caddy.nix adds TLS and a no-script CSP.
let
  repo = "/srv/lab-git/stasis.git";
  guide = "src/main/resources/assets/stasisdelvers/guides/eln/handbook";
  out = "/srv/eln-handbook";

  render = pkgs.writers.writePython3 "eln-handbook-render"
    {
      libraries = [ pkgs.python3Packages.markdown pkgs.python3Packages.pyyaml ];
      flakeIgnore = [ "E501" ];
    }
    (builtins.readFile ./eln-handbook.py);
in
{
  users.users.eln-handbook = {
    isSystemUser = true;
    group = "eln-handbook";
  };
  users.groups.eln-handbook = { };

  # caddy-static reads the site as "other".
  systemd.tmpfiles.rules = [ "d ${out} 0755 eln-handbook eln-handbook -" ];

  systemd.services.eln-handbook = {
    description = "Render the EA handbook from Stasis main for eln.brage.info";
    path = [ pkgs.git pkgs.gnutar pkgs.coreutils pkgs.rsync ];
    unitConfig.RequiresMountsFor = [ "/srv/lab-git" out ];
    serviceConfig = {
      Type = "oneshot";
      User = "eln-handbook";
      Group = "eln-handbook";
      # Read access to the lab's shared repos (2770 root:lab-agents); the mount is read-only below.
      SupplementaryGroups = [ "lab-agents" ];
      ReadOnlyPaths = [ "/srv/lab-git" ];
      ReadWritePaths = [ out ];
      UMask = "0022";
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      PrivateDevices = true;
      PrivateNetwork = true;
      ProtectProc = "invisible";
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      ProtectControlGroups = true;
      RestrictAddressFamilies = [ "AF_UNIX" ];
      CapabilityBoundingSet = "";
      SystemCallFilter = [ "@system-service" ];
      MemoryMax = "1G";
    };
    script = ''
      set -euo pipefail
      export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0=${repo}
      if [ ! -d ${repo} ]; then
        echo "no ${repo}; nothing to render"
        exit 0
      fi
      rev=$(git --git-dir=${repo} rev-parse --verify 'main^{commit}')
      if [ -d ${out}/site ] && [ "$(cat ${out}/rev 2>/dev/null || true)" = "$rev" ]; then
        exit 0
      fi
      tmp=$(mktemp -d)
      trap 'rm -rf "$tmp"' EXIT
      mkdir "$tmp/src" "$tmp/site"
      git --git-dir=${repo} archive "$rev" ${guide} | tar -x -C "$tmp/src"
      ${render} "$tmp/src/${guide}" "$tmp/site" "$rev"
      rsync -r --delete --chmod=D755,F644 "$tmp/site/" ${out}/site/
      echo "$rev" > ${out}/rev
      echo "rendered Stasis $rev"
    '';
  };

  systemd.timers.eln-handbook = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "2min";
      OnUnitActiveSec = "5min";
    };
  };
}
