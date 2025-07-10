{
  # Rules for naming various devices.
  # These are shared between all my machines so it doesn't matter where I plug them in.
  services.udev.extraRules = ''
    # Ten64 console port
    SUBSYSTEM=="tty", ATTRS{serial}=="D307YO1A", SYMLINK+="ttyUSB.ten64"
  '';
}
