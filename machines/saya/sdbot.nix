{ inputs, pkgs, ... }:

let
  # Updated CUDA and library paths for current nixpkgs
  cp = pkgs.cudaPackages_12_8;
  cudaLibPath = "/run/opengl-driver/lib:" + (with pkgs; lib.makeLibraryPath [
    cp.cudatoolkit
    cp.cudnn
    stdenv.cc.cc.lib
    libGL
    libGLU
    mesa
    glib
    zlib
    libffi
    openssl
    blas
    lapack
    hdf5
    ffmpeg
    portaudio
    libsndfile
    libjpeg
    libpng
    libtiff
    libwebp
  ]);

  COMFYUI_DIR = "/home/svein/AI/image-generation/ComfyUI";
  GANBOT_DIR = "/home/svein/dev/ganbot";

  # ComfyUI wrapper script that replicates shell.nix environment
  comfyui-wrapper = pkgs.writeShellScript "comfyui-wrapper" ''
    set -euo pipefail
    cd ${COMFYUI_DIR}

    # Set up environment variables like shell.nix
    export LD_LIBRARY_PATH="${cudaLibPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export CUDA_HOME="${cp.cudatoolkit}"
    export CUDA_PATH="${cp.cudatoolkit}"
    export CUDNN_PATH="${cp.cudnn}"
    export PYTORCH_CUDA_ALLOC_CONF="max_split_size_mb:512"
    export CUDA_VISIBLE_DEVICES=0

    # Create virtual environment if it doesn't exist
    if [[ ! -d .venv ]]; then
      echo "Creating virtual environment..."
      ${pkgs.uv}/bin/uv venv --prompt comfyui --python 3.12 --python-preference only-managed .venv

      source .venv/bin/activate

      echo "Installing PyTorch with CUDA support..."
      ${pkgs.uv}/bin/uv pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu124

      echo "Installing ComfyUI requirements..."
      ${pkgs.uv}/bin/uv pip install -r requirements.txt
    fi

    source .venv/bin/activate

    echo "Starting ComfyUI..."
    exec python main.py --listen --port 8188
  '';

  # Background process manager wrapper for ganbot
  background-process-manager-wrapper = pkgs.writeShellScript "background-process-manager-wrapper" ''
    set -euo pipefail
    export RUST_BACKTRACE=1
    export RUST_LOG="background_process_manager=info,ganbot=debug"

    exec ${inputs.background-process-manager.apps.x86_64-linux.default.program} ${GANBOT_DIR}
  '';

  comfyui-service = {
    description = "ComfyUI Stable Diffusion Interface";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    wants = [ "network-online.target" ];

    path = with pkgs; [
      python312
      uv
      git
      gcc
      gnumake
      cmake
      pkg-config
      cp.cudatoolkit
      cp.cudnn
      steam-run
    ];

    serviceConfig = {
      User = "svein";
      Group = "users";
      WorkingDirectory = COMFYUI_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = "10s";
      ExecStart = comfyui-wrapper;
      MemoryMax = "64G";
      RuntimeMaxSec = "72h";

      # Performance optimizations
      Environment = [
        "LD_PRELOAD=${pkgs.gperftools}/lib/libtcmalloc.so"
        "PYTORCH_CUDA_ALLOC_CONF=max_split_size_mb:512"
      ];

      # Security restrictions
      PrivateTmp = true;
      ProtectSystem = "strict";
      ReadWritePaths = [ COMFYUI_DIR ];
      NoNewPrivileges = true;
    };
  };

  ganbot-service = {
    description = "Background Process Manager for Ganbot";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "comfyui.service" ];
    wants = [ "network-online.target" ];

    path = with pkgs; [
      openssh
      # Tools needed by background-process-manager
      cargo
      rustc
      direnv
      git
    ];

    serviceConfig = {
      User = "svein";
      Group = "users";
      WorkingDirectory = GANBOT_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = "10s";
      ExecStart = background-process-manager-wrapper;

      # Security restrictions
      PrivateTmp = true;
      ProtectSystem = "strict";
      ReadWritePaths = [ GANBOT_DIR ];
      NoNewPrivileges = true;
    };
  };
in
{
  systemd.services.comfyui = comfyui-service;
  systemd.services.ganbot = ganbot-service;
  environment.systemPackages = [ inputs.background-process-manager.packages.x86_64-linux.bpm-tui ];

  # Port 8188 for ComfyUI, 8485 for ganbot, 3001 for MCP server
  networking.firewall.allowedTCPPorts = [ 8188 8485 3001 ];
}
