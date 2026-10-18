{ lib, pkgs, ganbot, ... }:

let
  ganbotPackage = ganbot.packages.x86_64-linux.default.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./patches/ganbot-image-limits.patch ];
  });
in
{
  systemd.services.ganbot = {
    description = "Ganbot Discord/IRC bot";
    after = [ "network-online.target" "wireguard-wg1.target" ];
    wants = [ "network-online.target" "wireguard-wg1.target" ];
    wantedBy = [ "multi-user.target" ];
    path = [ pkgs.openssh ];
    serviceConfig = {
      Type = "simple";
      User = "svein";
      WorkingDirectory = "/home/svein/dev/ganbot";
      ExecStart = "${ganbotPackage}/bin/ganbot";
      Restart = "on-failure";
      RestartSec = "5s";
      StateDirectory = "ganbot";
      Environment = [
        "XDG_DATA_HOME=/var/lib/ganbot"
        "RUST_LOG=warn,ganbot=info"
      ];

      # Hardening
      ProtectSystem = "strict";
      ProtectHome = "tmpfs";
      BindReadOnlyPaths = [ "/nix/store" "/home/svein/dev/ganbot" "/home/svein/.ssh" ];
      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectKernelTunables = true;
      ProtectControlGroups = true;
      RestrictSUIDSGID = true;
      MemoryMax = "2G";
    };
  };

  # ganbot.brage.info
  networking.firewall.allowedTCPPorts = [ 8485 ];

  systemd.services.comfyui = {
    description = "ComfyUI AI Image Generation";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    # ComfyUI-Manager imports GitPython, which needs git at startup.
    path = [ pkgs.git ];
    # Point the venv at this config's python313, so it matches the glibc of
    # the LD_LIBRARY_PATH libs below and is GC-rooted by the system closure.
    preStart = ''
      ln -sfn ${pkgs.python313}/bin/python3 .venv/bin/python
      sed -i -e 's|^home = .*|home = ${pkgs.python313}/bin|' \
             -e 's|^version_info = .*|version_info = ${pkgs.python313.version}|' .venv/pyvenv.cfg
    '';
    serviceConfig = {
      Type = "simple";
      User = "svein";
      WorkingDirectory = "/home/svein/ai/comfy";
      ExecStart = "/home/svein/ai/comfy/.venv/bin/python main.py --extra-model-paths-config extra_model_paths.yaml --enable-manager";
      Environment = [
        # Runtime deps of the pip wheels: opencv (custom nodes) and ANGLE (nodes_glsl).
        "LD_LIBRARY_PATH=/run/opengl-driver/lib:${lib.makeLibraryPath (with pkgs; [
          stdenv.cc.cc.lib zlib glib libglvnd libx11 libxext libxcb
        ])}"
        # ComfyUI-Manager shells out to `python -m uv pip`; ~/.cache is
        # read-only under ProtectHome. The cache is on another filesystem
        # than the venv, so copy instead of hardlinking.
        "UV_CACHE_DIR=/var/cache/comfyui/uv"
        "UV_LINK_MODE=copy"
      ];
      Restart = "on-failure";
      RestartSec = "10s";
      CacheDirectory = "comfyui";

      # Hardening
      ProtectSystem = "strict";
      ProtectHome = "tmpfs";
      BindPaths = [ "/home/svein/ai/comfy" ];
      BindReadOnlyPaths = [ "/home/svein/ai/models" "/nix/store" ];
      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectKernelTunables = true;
      ProtectControlGroups = true;
      RestrictSUIDSGID = true;
    };
  };
}
