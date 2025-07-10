{ config
, pkgs
, ...
}: {
  # In theory it should be possible to run hostapd under NixOS, but the hardware
  # isn't cooperating. OpenWRT carries many patches. It's all rather concerning.

  #boot.kernelPatches = [{
  #  name = "ath10k";
  #  patch = null;
  #  extraConfig = ''
  #    EXPERT y
  #    CFG80211_CERTIFICATION_ONUS y
  #    ATH10K_DFS_CERTIFIED y
  #  '';
  #}];

  # So let's run OpenWRT in qemu instead.
  me = {
    virtualisation.enable = true;
  };

  # Pass the WiFi card through to OpenWRT.
  boot.kernelParams = [ "amd_iommu=on" ];
  boot.kernelModules = [ "vfio" "vfio_iommu_type1" "vfio_pci" "vfio_virqfd" ];
  boot.extraModprobeConfig = "options vfio-pci ids=168c:0046";

  # Wifi AP
  #services.udev.packages = [ pkgs.crda ];
  #environment.systemPackages = [ pkgs.iw pkgs.crda ];
  #hardware.firmware = [ pkgs.wireless-regdb ];
  #systemd.services.hostapd = let hostapd = pkgs.hostapd.overrideAttrs (oldAttrs: {
  #  src = builtins.fetchGit {
  #    url = http://w1.fi/hostap.git;
  #    #rev = "5a8b366233f5585e68a4ffbb604fbb4a848eb325";
  #    ref = "master";
  #  };
  #  patches = null;
  #}); in {
  #  wantedBy = [ "multi-user.target" ];
  #  after = [ "network.target" ];
  #  description = "Hostapd service";
  #  serviceConfig = {
  #    Type = "simple";
  #    User = "root";
  #    ExecStart = ''${hostapd}/bin/hostapd ${./hostapd.conf}'';
  #  };
  #};
}
