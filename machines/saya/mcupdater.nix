{ pkgs, ... }:

let
  # mcupdater [bootstrap.jar] — run the MCUpdater bootstrap (default
  # ~/.MCUpdater/MCU-Bootstrap.jar).
  #
  # The bootstrap re-extracts its own Temurin runtimes into ~/.MCUpdater/runtime
  # on every start, so it can't run on one of those (ETXTBSY) and patchelf
  # wouldn't stick; it runs on a nixpkgs JDK, and the downloaded JDKs, JavaFX
  # and Minecraft find their libraries through nix-ld below.
  #
  # -XX:-UseContainerSupport: the GUI runs on the bundled jdk-17+35 (17 GA),
  # which NPEs in its cgroup v2 detection. JDK_JAVA_OPTIONS reaches every
  # child java >= 9; the bootstrap passes its environment through.
  mcupdater = pkgs.writeShellApplication {
    name = "mcupdater";
    text = ''
      jar="''${1:-$HOME/.MCUpdater/MCU-Bootstrap.jar}"
      export JDK_JAVA_OPTIONS="-XX:-UseContainerSupport ''${JDK_JAVA_OPTIONS:-}"
      cd "$(dirname "$jar")"
      exec ${pkgs.temurin-bin-21}/bin/java -jar "$jar"
    '';
  };
in
{
  environment.systemPackages = [ mcupdater ];

  programs.nix-ld.libraries = with pkgs; [
    # AWT/Swing (bootstrap window)
    libx11 libxext libxrender libxtst libxi libxrandr libxcursor libxxf86vm
    libxinerama libxcomposite libxdamage libxfixes libxcb
    freetype fontconfig zlib alsa-lib
    # JavaFX (MCUpdater GUI)
    gtk3 glib pango cairo gdk-pixbuf atk harfbuzz libGL libxkbcommon
    gst_all_1.gstreamer gst_all_1.gst-plugins-base libxslt libxml2
    # Minecraft / LWJGL
    openal libpulseaudio flite udev wayland
  ];
}
