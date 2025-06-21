{
  # Work around https://unix.stackexchange.com/questions/743820/what-could-cause-a-missing-mouse-scroll-event-just-after-reversing-scroll-direct
  environment.etc."libinput/local-overrides.quirks".text = ''
    [Logitech G903 LS]
    MatchName=Logitech G903 LS
    AttrEventCode=-REL_WHEEL_HI_RES;
  '';
}
