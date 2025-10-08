{ inputs, pkgs, ... }:

let
  # Updated CUDA and library paths for current nixpkgs
  cudaLibPath = "/run/opengl-driver/lib:" + (with pkgs; lib.makeLibraryPath [
    cudaPackages.cudatoolkit
    cudaPackages.cudnn
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
    export CUDA_HOME="${pkgs.cudaPackages.cudatoolkit}"
    export CUDA_PATH="${pkgs.cudaPackages.cudatoolkit}"
    export CUDNN_PATH="${pkgs.cudaPackages.cudnn}"
    export PYTORCH_CUDA_ALLOC_CONF="max_split_size_mb:512"

    # Create virtual environment if it doesn't exist
    if [[ ! -d .venv ]]; then
      echo "Creating virtual environment..."
      ${pkgs.uv}/bin/uv venv --prompt comfyui --python 3.12 --python-preference only-managed .venv

      source .venv/bin/activate

      echo "Installing PyTorch with CUDA 12.4 support..."
      ${pkgs.uv}/bin/uv pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu124

      echo "Installing ComfyUI requirements..."
      ${pkgs.uv}/bin/uv pip install -r requirements.txt
    fi

    source .venv/bin/activate

    echo "Starting ComfyUI..."
    exec python main.py --listen --port 8188
  '';

  # Ganbot wrapper that uses the development environment
  ganbot-wrapper = pkgs.writeShellScript "ganbot-wrapper" ''
    set -euo pipefail
    cd ${GANBOT_DIR}

    export RUST_BACKTRACE=1
    export RUST_LOG="ganbot=debug"

    exec ${inputs.ganbot.apps.x86_64-linux.default.program}
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
      cudaPackages.cudatoolkit
      cudaPackages.cudnn
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
    description = "Ganbot - Multi-platform Discord/IRC Bot";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "comfyui.service" ];
    wants = [ "network-online.target" ];

    path = with pkgs; [
      openssh
    ];

    serviceConfig = {
      User = "svein";
      Group = "users";
      WorkingDirectory = GANBOT_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = "10s";
      ExecStart = ganbot-wrapper;

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
  #systemd.services.ganbot = ganbot-service;

  networking.firewall.allowedTCPPorts = [ 8188 8485 ];
}
