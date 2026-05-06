{ pkgs, ganbot, ... }:

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
      ExecStart = "${ganbot.packages.x86_64-linux.default}/bin/ganbot";
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
    };
  };

  # ganbot.brage.info
  networking.firewall.allowedTCPPorts = [ 8485 ];

  systemd.services.comfyui = {
    description = "ComfyUI AI Image Generation";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      User = "svein";
      WorkingDirectory = "/home/svein/ai/comfy";
      ExecStart = "/home/svein/ai/comfy/.venv/bin/python main.py --extra-model-paths-config extra_model_paths.yaml";
      Environment = [
        "LD_LIBRARY_PATH=/run/opengl-driver/lib:${pkgs.stdenv.cc.cc.lib}/lib"
      ];
      Restart = "on-failure";
      RestartSec = "10s";

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
