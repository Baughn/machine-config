let
  # Last working at revision 8726c6d50638fea25808e99192ed546657200d87
  minecraft-pkgs = import /home/svein/dev/nix-mcupdater {};

  minecraft = with minecraft-pkgs; with stdenv; rec {

    libraries = with xlibs; lib.makeLibraryPath [
      stdenv.cc.cc libX11 libXext libXcursor libXrandr libXxf86vm mesa openal libpulseaudio
    ];

    openalLib = lib.makeLibraryPath [ openal ];

    mcenv = mkDerivation {
      name = "mcenv-2";

      inherit openalLib libraries;

      phases = ["installPhase"];
      installPhase = ''
        mkdir -p $out/bin
        cat > $out/bin/mcenv << EOF
          #!${stdenv.shell}
          # wrapper for mcupdater/minecraft
          export LD_LIBRARY_PATH=\$LD_LIBRARY_PATH:$libraries
          export LD_PRELOAD=$openalLib/libopenal.so
          source ${jdk}/nix-support/setup-hook
          export PATH=$PATH:${jdk}/bin
        EOF
        chmod a+x $out/bin/mcenv
      '';
    };

    mkMCDerivation = self: mkDerivation ({
      phases = "installPhase";

      installPhase = ''
        mkdir -p $out/bin
        cat > $out/bin/$name << EOF
          #!${stdenv.shell}
          source ${mcenv}/bin/mcenv
          ${jdk}/bin/java -jar $src "\$@"
        EOF
        chmod a+x $out/bin/$name
      '';
    } // self);

    mcupdater = mkMCDerivation {
      name = "mcupdater";

      src = fetchurl {
        url = https://madoka.brage.info/MCU-Bootstrap.jar;
        sha256 = "1rm287bf4m0lnxc7yk5ahrmbbqnrp3ywq7ig5wm3wc5zpsjpfp0n";
      };
    };

    ftb-launcher = mkMCDerivation {
      name = "ftb-launcher";
      src = /home/svein/FTB/FTB_Launcher.jar;
    };

    # atlauncher = mkMCDerivation {
    #   name = "atlauncher";
    #   src = /home/svein/atlauncher/ATLauncher.jar;
    # };

    mc-fhs = buildFHSUserEnv {
      name = "mc-fhs";

      targetPkgs = pkgs: with pkgs; with xlibs; [
        firefox xdg_utils zsh jdk libX11 libXext libXcursor libXrandr libXxf86vm mesa openal
      ];

      runScript = "zsh";

      profile = ''
      '';
    };
  };
in

{ 
  allowUnfree = true; 

  packageOverrides = pkgs: rec {
    mcenv = minecraft.mcenv;
    mcupdater = minecraft.mcupdater;
    ftb-launcher = minecraft.ftb-launcher;
    # atlauncher = minecraft.atlauncher;

    mc-fhs = minecraft.mc-fhs;

    rimworld = (import "/home/svein/My Games/Rimworld").rimworld;

    idea = with pkgs; with xlibs; stdenv.mkDerivation {
      name = "idea-2";

      buildInputs = [ makeWrapper ];

      phases = "installPhase";

      libraries = mcupdater.libraries;

      installPhase = ''
        mkdir $out/bin -p
        makeWrapper ${pkgs.idea.idea-ultimate}/bin/idea-ultimate $out/bin/idea \
          --suffix LD_LIBRARY_PATH : $libraries
      '';
    };

    factorio = pkgs.factorio.override {
      username = "sveina@gmail.com";
      password = "dkVe3WEQn9KkQC0e";
    };
  };
}
