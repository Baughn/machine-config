# CachyOS-flavoured kernel and matching system tweaks.
#
# Wraps the Zen 4 optimized CachyOS kernel variant from
# xddxdd/nix-cachyos-kernel, overrides its structuredExtraConfig to drop
# modules that no machine here uses (legacy wireless, SAS/FC/iSCSI HBAs,
# media capture, virtual-guest DRMs, embedded panel bridges, etc.), and
# applies the CachyOS-style sysctl/systemd/udev tunings that go with it.
#
# Gated by `me.cachy-kernel.enable`. Per-host knobs:
#   me.cachy-kernel.amdgpu     — keep AMDGPU enabled and unblacklisted
#                                (default true; saya sets false because
#                                its iGPU is intentionally dark behind
#                                an NVIDIA dGPU).
#   me.cachy-kernel.scheduler  — sched-ext BPF scheduler, or null.

{ config, lib, pkgs, ... }:

let
  cfg = config.me.cachy-kernel;

  baseKernelPackages = pkgs.cachyosKernels.linuxPackages-cachyos-latest-zen4;

  disabledKernelConfigPrefixes = [
    "ARCNET"
    "ATH"
    "B43"
    "B43LEGACY"
    "BRCM"
    "BT"
    "CFG80211"
    "CHROME"
    "CHROMEOS"
    "COMEDI"
    "DRM_ANALOGIX"
    "DRM_AUX"
    "DRM_CDNS"
    "DRM_CHIPONE"
    "DRM_CHRONTEL"
    "DRM_ETNAVIV"
    "DRM_GUD"
    "DRM_HISI"
    "DRM_I2C_ADV"
    "DRM_I2C_NXP"
    "DRM_ITE"
    "DRM_KOMEDA"
    "DRM_LOGICVC"
    "DRM_LONTIUM"
    "DRM_LVDS_CODEC"
    "DRM_MEGACHIPS"
    "DRM_NWL"
    "DRM_PARADE"
    "DRM_SAMSUNG_DSIM"
    "DRM_SII"
    "DRM_SITRONIX"
    "DRM_SOLOMON"
    "DRM_SIMPLE_BRIDGE"
    "DRM_THINE"
    "DRM_TI"
    "DRM_TINY"
    "DRM_TOSHIBA"
    "DRM_WAVESHARE"
    "FB_TFT"
    "GREYBUS"
    "HERMES"
    "HOSTAP"
    "IEEE802154"
    "INFINIBAND"
    "IIO"
    "IP_SET"
    "IP_VS"
    "ISDN"
    "IWLEGACY"
    "IWL"
    "LIBERTAS"
    "MAC80211"
    "MISDN"
    "MT76"
    "MT7921"
    "MTD"
    "MWIFIEX"
    "MWL"
    "NET_DSA"
    "NFC"
    "ORINOCO"
    "P54"
    "PMBUS"
    "QTNFMAC"
    "R818"
    "R819"
    "RDMA"
    "RSI"
    "RTL"
    "RTW"
    "SND_SOC"
    "SND_SOF"
    "STAGING"
    "SURFACE"
    "USB_SERIAL"
    "WIL6210"
    "WLAN"
    "WL12"
    "WL1251"
    "WWAN"
    "ZD1211RW"
  ];

  isDisabledKernelConfig = name:
    lib.any
      (prefix: name == prefix || lib.hasPrefix "${prefix}_" name)
      disabledKernelConfigPrefixes;

  baseStructuredExtraConfig =
    lib.filterAttrs
      (name: _: ! isDisabledKernelConfig name)
      baseKernelPackages.kernel.structuredExtraConfig;

  baseConfigOptionNames =
    lib.pipe (lib.splitString "\n" (builtins.readFile baseKernelPackages.kernel.cachyosConfigFile)) [
      (map (line:
        let
          match = builtins.match "CONFIG_([A-Z0-9_]+)=.*" line;
        in
          if match == null then null else builtins.head match))
      (lib.filter (name: name != null && isDisabledKernelConfig name))
    ];

  drmPanelBridgeOptions = [
    "DRM_ANALOGIX_ANX6345"
    "DRM_ANALOGIX_ANX7625"
    "DRM_ANALOGIX_DP"
    "DRM_AUX_BRIDGE"
    "DRM_AUX_HPD_BRIDGE"
    "DRM_CDNS_DSI"
    "DRM_CDNS_DSI_J721E"
    "DRM_CDNS_MHDP8546"
    "DRM_CHIPONE_ICN6211"
    "DRM_CHRONTEL_CH7033"
    "DRM_ITE_IT6263"
    "DRM_ITE_IT6505"
    "DRM_ITE_IT66121"
    "DRM_LONTIUM_LT8912B"
    "DRM_LONTIUM_LT9211"
    "DRM_LONTIUM_LT9611"
    "DRM_LONTIUM_LT9611UXC"
    "DRM_LVDS_CODEC"
    "DRM_MEGACHIPS_STDPXXXX_GE_B850V3_FW"
    "DRM_NWL_MIPI_DSI"
    "DRM_PANEL_ABT_Y030XX067A"
    "DRM_PANEL_ARM_VERSATILE"
    "DRM_PANEL_ASUS_Z00T_TM5P5_NT35596"
    "DRM_PANEL_AUO_A030JTN01"
    "DRM_PANEL_BOE_BF060Y8M_AJ0"
    "DRM_PANEL_BOE_HIMAX8279D"
    "DRM_PANEL_BOE_TD4320"
    "DRM_PANEL_BOE_TH101MB31UIG002_28A"
    "DRM_PANEL_BOE_TV101WUM_LL2"
    "DRM_PANEL_BOE_TV101WUM_NL6"
    "DRM_PANEL_DSI_CM"
    "DRM_PANEL_EBBG_FT8719"
    "DRM_PANEL_EDP"
    "DRM_PANEL_ELIDA_KD35T133"
    "DRM_PANEL_FEIXIN_K101_IM2BA02"
    "DRM_PANEL_FEIYANG_FY07024DI26A30D"
    "DRM_PANEL_HIMAX_HX8279"
    "DRM_PANEL_HIMAX_HX83102"
    "DRM_PANEL_HIMAX_HX83112A"
    "DRM_PANEL_HIMAX_HX83112B"
    "DRM_PANEL_HIMAX_HX8394"
    "DRM_PANEL_HYDIS_HV101HD1"
    "DRM_PANEL_ILITEK_IL9322"
    "DRM_PANEL_ILITEK_ILI9341"
    "DRM_PANEL_ILITEK_ILI9805"
    "DRM_PANEL_ILITEK_ILI9806E"
    "DRM_PANEL_ILITEK_ILI9881C"
    "DRM_PANEL_ILITEK_ILI9882T"
    "DRM_PANEL_INNOLUX_EJ030NA"
    "DRM_PANEL_INNOLUX_P079ZCA"
    "DRM_PANEL_JADARD_JD9365DA_H3"
    "DRM_PANEL_JDI_LPM102A188A"
    "DRM_PANEL_JDI_LT070ME05000"
    "DRM_PANEL_JDI_R63452"
    "DRM_PANEL_KHADAS_TS050"
    "DRM_PANEL_KINGDISPLAY_KD097D04"
    "DRM_PANEL_LEADTEK_LTK050H3146W"
    "DRM_PANEL_LEADTEK_LTK500HD1829"
    "DRM_PANEL_LG_LB035Q02"
    "DRM_PANEL_LG_LD070WX3"
    "DRM_PANEL_LG_LG4573"
    "DRM_PANEL_LG_SW43408"
    "DRM_PANEL_LINCOLNTECH_LCD197"
    "DRM_PANEL_LVDS"
    "DRM_PANEL_MAGNACHIP_D53E6EA8966"
    "DRM_PANEL_MANTIX_MLAF057WE51"
    "DRM_PANEL_NEC_NL8048HL11"
    "DRM_PANEL_NEWVISION_NV3051D"
    "DRM_PANEL_NEWVISION_NV3052C"
    "DRM_PANEL_NOVATEK_NT35510"
    "DRM_PANEL_NOVATEK_NT35560"
    "DRM_PANEL_NOVATEK_NT35950"
    "DRM_PANEL_NOVATEK_NT36523"
    "DRM_PANEL_NOVATEK_NT36672A"
    "DRM_PANEL_NOVATEK_NT36672E"
    "DRM_PANEL_NOVATEK_NT37801"
    "DRM_PANEL_NOVATEK_NT39016"
    "DRM_PANEL_OLIMEX_LCD_OLINUXINO"
    "DRM_PANEL_ORISETECH_OTA5601A"
    "DRM_PANEL_ORISETECH_OTM8009A"
    "DRM_PANEL_OSD_OSD101T2587_53TS"
    "DRM_PANEL_PANASONIC_VVX10F034N00"
    "DRM_PANEL_RASPBERRYPI_TOUCHSCREEN"
    "DRM_PANEL_RAYDIUM_RM67191"
    "DRM_PANEL_RAYDIUM_RM67200"
    "DRM_PANEL_RAYDIUM_RM68200"
    "DRM_PANEL_RAYDIUM_RM692E5"
    "DRM_PANEL_RAYDIUM_RM69380"
    "DRM_PANEL_RENESAS_R61307"
    "DRM_PANEL_RENESAS_R69328"
    "DRM_PANEL_RONBO_RB070D30"
    "DRM_PANEL_SAMSUNG_AMS581VF01"
    "DRM_PANEL_SAMSUNG_AMS639RQ08"
    "DRM_PANEL_SAMSUNG_ATNA33XC20"
    "DRM_PANEL_SAMSUNG_DB7430"
    "DRM_PANEL_SAMSUNG_LD9040"
    "DRM_PANEL_SAMSUNG_LTL106HL02"
    "DRM_PANEL_SAMSUNG_S6D16D0"
    "DRM_PANEL_SAMSUNG_S6D27A1"
    "DRM_PANEL_SAMSUNG_S6D7AA0"
    "DRM_PANEL_SAMSUNG_S6E3FA7"
    "DRM_PANEL_SAMSUNG_S6E3FC2X01"
    "DRM_PANEL_SAMSUNG_S6E3HA2"
    "DRM_PANEL_SAMSUNG_S6E3HA8"
    "DRM_PANEL_SAMSUNG_S6E63J0X03"
    "DRM_PANEL_SAMSUNG_S6E63M0"
    "DRM_PANEL_SAMSUNG_S6E63M0_DSI"
    "DRM_PANEL_SAMSUNG_S6E63M0_SPI"
    "DRM_PANEL_SAMSUNG_S6E88A0_AMS427AP24"
    "DRM_PANEL_SAMSUNG_S6E88A0_AMS452EF01"
    "DRM_PANEL_SAMSUNG_S6E8AA0"
    "DRM_PANEL_SAMSUNG_S6E8AA5X01_AMS561RA01"
    "DRM_PANEL_SAMSUNG_SOFEF00"
    "DRM_PANEL_SEIKO_43WVF1G"
    "DRM_PANEL_SHARP_LQ079L1SX01"
    "DRM_PANEL_SHARP_LQ101R1SX01"
    "DRM_PANEL_SHARP_LS037V7DW01"
    "DRM_PANEL_SHARP_LS043T1LE01"
    "DRM_PANEL_SHARP_LS060T1SX01"
    "DRM_PANEL_SIMPLE"
    "DRM_PANEL_SITRONIX_ST7701"
    "DRM_PANEL_SITRONIX_ST7703"
    "DRM_PANEL_SITRONIX_ST7789V"
    "DRM_PANEL_SONY_ACX565AKM"
    "DRM_PANEL_SONY_TD4353_JDI"
    "DRM_PANEL_SONY_TULIP_TRULY_NT35521"
    "DRM_PANEL_STARTEK_KD070FHFID015"
    "DRM_PANEL_SUMMIT"
    "DRM_PANEL_SYNAPTICS_R63353"
    "DRM_PANEL_SYNAPTICS_TDDI"
    "DRM_PANEL_TDO_TL070WSH30"
    "DRM_PANEL_TPO_TD028TTEC1"
    "DRM_PANEL_TPO_TD043MTEA1"
    "DRM_PANEL_TPO_TPG110"
    "DRM_PANEL_TRULY_NT35597_WQXGA"
    "DRM_PANEL_VISIONOX_G2647FB105"
    "DRM_PANEL_VISIONOX_R66451"
    "DRM_PANEL_VISIONOX_RM69299"
    "DRM_PANEL_VISIONOX_RM692E5"
    "DRM_PANEL_VISIONOX_VTDR6130"
    "DRM_PANEL_WIDECHIPS_WS2401"
    "DRM_PANEL_XINPENG_XPP055C272"
    "DRM_PARADE_PS8622"
    "DRM_PARADE_PS8640"
    "DRM_SAMSUNG_DSIM"
    "DRM_SII902X"
    "DRM_SII9234"
    "DRM_SIMPLE_BRIDGE"
    "DRM_SOLOMON_SSD2825"
    "DRM_THINE_THC63LVD1024"
    "DRM_TI_DLPC3433"
    "DRM_TI_SN65DSI83"
    "DRM_TI_SN65DSI86"
    "DRM_TI_TDP158"
    "DRM_TI_TFP410"
    "DRM_TI_TPD12S015"
    "DRM_TOSHIBA_TC358762"
    "DRM_TOSHIBA_TC358764"
    "DRM_TOSHIBA_TC358767"
    "DRM_TOSHIBA_TC358768"
    "DRM_TOSHIBA_TC358775"
    "DRM_WAVESHARE_BRIDGE"
  ];

  forceDisabledKernelOptions =
    lib.genAttrs (lib.unique (baseConfigOptionNames ++ drmPanelBridgeOptions ++ [
      "ARCNET"
      "BT"
      "CFG80211"
      "CHROME_PLATFORMS"
      "COMEDI"
      "DRM_DISPLAY_CONNECTOR"
      "DRM_PANEL_MIPI_DBI"
      "DRM_TINY"
      "FB_TFT"
      "GREYBUS"
      "IEEE802154"
      "INFINIBAND"
      "IIO"
      "IP_SET"
      "IP_VS"
      "ISDN"
      "MAC80211"
      "MTD"
      "NET_DSA"
      "NFC"
      "PMBUS"
      "PHY_QCOM_QMP_COMBO"
      "QCOM_PMIC_GLINK"
      "RDMA_RXE"
      "RDMA_SIW"
      "SND_SOC"
      "SND_SOF"
      "STAGING"
      "STAGING_MEDIA"
      "SURFACE_PLATFORMS"
      "TYPEC_MUX_NB7VPQ904M"
      "TYPEC_MUX_PS883X"
      "TYPEC_MUX_PTN36502"
      "UCSI_HUAWEI_GAOKUN"
      "UCSI_LENOVO_YOGA_C630"
      "USB_SERIAL"
      "WLAN"
      "WWAN"
    ]))
    (_: lib.mkForce lib.kernel.no);

  amdgpuKernelConfig = lib.optionalAttrs (! cfg.amdgpu) (with lib.kernel; {
    DRM_AMDGPU = lib.mkForce no;
  });

  customKernel = baseKernelPackages.kernel.override {
    kernelPatches = (baseKernelPackages.kernel.kernelPatches or []) ++ [
      {
        name = "afs-do-not-select-rxrpc";
        patch = ./patches/afs-do-not-select-rxrpc.patch;
      }
    ];
    structuredExtraConfig = baseStructuredExtraConfig // forceDisabledKernelOptions // amdgpuKernelConfig // (with lib.kernel; {
      # AF_ALG userspace crypto socket. CVE-2026-31431 is in algif_aead; the
      # rest of the family is disabled with it because no service here uses
      # AF_ALG.
      CRYPTO_USER_API = lib.mkForce no;
      CRYPTO_USER_API_AEAD = lib.mkForce no;
      CRYPTO_USER_API_HASH = lib.mkForce no;
      CRYPTO_USER_API_RNG = lib.mkForce no;
      CRYPTO_USER_API_SKCIPHER = lib.mkForce no;

      # Dirty Frag mitigation: IPsec ESP and RxRPC. WireGuard is used in
      # place of IPsec; no AFS/RxRPC consumers anywhere.
      INET_ESP = lib.mkForce no;
      INET6_ESP = lib.mkForce no;
      AF_RXRPC = lib.mkForce no;
      AFS_FS = lib.mkForce no;

      DRM_I915 = lib.mkForce no;
      DRM_XE = lib.mkForce no;
      DRM_NOUVEAU = lib.mkForce no;
      DRM_RADEON = lib.mkForce no;

      DRM_ACCEL_AMDXDNA = lib.mkForce no;
      DRM_ACCEL_HABANALABS = lib.mkForce no;
      DRM_ACCEL_IVPU = lib.mkForce no;
      DRM_ACCEL_QAIC = lib.mkForce no;

      # Obsolete and virtual display drivers. simpledrm/EFI framebuffer plus
      # the proprietary NVIDIA module path stay.
      DRM_AST = lib.mkForce no;
      DRM_BOCHS = lib.mkForce no;
      DRM_CIRRUS_QEMU = lib.mkForce no;
      DRM_GMA500 = lib.mkForce no;
      DRM_HYPERV = lib.mkForce no;
      DRM_MGAG200 = lib.mkForce no;
      DRM_QXL = lib.mkForce no;
      DRM_VBOXVIDEO = lib.mkForce no;
      DRM_VIRTIO_GPU = lib.mkForce no;
      DRM_VMWGFX = lib.mkForce no;
      DRM_XEN_FRONTEND = lib.mkForce no;

      FB_3DFX = lib.mkForce no;
      FB_ATY = lib.mkForce no;
      FB_ATY128 = lib.mkForce no;
      FB_CIRRUS = lib.mkForce no;
      FB_CYBER2000 = lib.mkForce no;
      FB_HGA = lib.mkForce no;
      FB_I740 = lib.mkForce no;
      FB_NVIDIA = lib.mkForce no;
      FB_RADEON = lib.mkForce no;
      FB_RIVA = lib.mkForce no;
      FB_SAVAGE = lib.mkForce no;
      FB_SIS = lib.mkForce no;
      FB_TRIDENT = lib.mkForce no;
      FB_VGA16 = lib.mkForce no;
      FB_VIA = lib.mkForce no;
      FB_VOODOO1 = lib.mkForce no;

      # Local PCIe NVMe only; no fabrics, target mode, or auth.
      NVME_AUTH = lib.mkForce no;
      NVME_FABRICS = lib.mkForce no;
      NVME_FC = lib.mkForce no;
      NVME_HOST_AUTH = lib.mkForce no;
      NVME_KEYRING = lib.mkForce no;
      NVME_RDMA = lib.mkForce no;
      NVME_TCP = lib.mkForce no;
      NVME_TARGET = lib.mkForce no;

      # Keep SCSI core/libata for SATA and USB mass storage. Drop legacy and
      # server-grade SCSI, SAS, Fibre Channel, iSCSI, and RAID adapters.
      ATA_OVER_ETH = lib.mkForce no;
      SCSI_LOWLEVEL = lib.mkForce no;
      FUSION = lib.mkForce no;
      FUSION_FC = lib.mkForce no;
      FUSION_LAN = lib.mkForce no;
      FUSION_SAS = lib.mkForce no;
      FUSION_SPI = lib.mkForce no;
      MEGARAID_LEGACY = lib.mkForce no;
      MEGARAID_MAILBOX = lib.mkForce no;
      MEGARAID_MM = lib.mkForce no;
      MEGARAID_SAS = lib.mkForce no;
      SCSI_3W_9XXX = lib.mkForce no;
      SCSI_3W_SAS = lib.mkForce no;
      SCSI_AACRAID = lib.mkForce no;
      SCSI_ACARD = lib.mkForce no;
      SCSI_ADVANSYS = lib.mkForce no;
      SCSI_AIC7XXX = lib.mkForce no;
      SCSI_AIC79XX = lib.mkForce no;
      SCSI_AIC94XX = lib.mkForce no;
      SCSI_AM53C974 = lib.mkForce no;
      SCSI_ARCMSR = lib.mkForce no;
      SCSI_BFA_FC = lib.mkForce no;
      SCSI_BNX2_ISCSI = lib.mkForce no;
      SCSI_BNX2X_FCOE = lib.mkForce no;
      SCSI_BUSLOGIC = lib.mkForce no;
      SCSI_CHELSIO_FCOE = lib.mkForce no;
      SCSI_CXGB3_ISCSI = lib.mkForce no;
      SCSI_CXGB4_ISCSI = lib.mkForce no;
      SCSI_DC395x = lib.mkForce no;
      SCSI_DMX3191D = lib.mkForce no;
      SCSI_EFCT = lib.mkForce no;
      SCSI_ESAS2R = lib.mkForce no;
      SCSI_FDOMAIN = lib.mkForce no;
      SCSI_HPSA = lib.mkForce no;
      SCSI_HPTIOP = lib.mkForce no;
      SCSI_INIA100 = lib.mkForce no;
      SCSI_INITIO = lib.mkForce no;
      SCSI_IPR = lib.mkForce no;
      SCSI_IPS = lib.mkForce no;
      SCSI_ISCI = lib.mkForce no;
      SCSI_LPFC = lib.mkForce no;
      SCSI_MPT2SAS = lib.mkForce no;
      SCSI_MPT3SAS = lib.mkForce no;
      SCSI_MVSAS = lib.mkForce no;
      SCSI_MVUMI = lib.mkForce no;
      SCSI_MYRB = lib.mkForce no;
      SCSI_MYRS = lib.mkForce no;
      SCSI_PM8001 = lib.mkForce no;
      SCSI_PMCRAID = lib.mkForce no;
      SCSI_PPA = lib.mkForce no;
      SCSI_QLA_FC = lib.mkForce no;
      SCSI_QLA_ISCSI = lib.mkForce no;
      SCSI_QLOGIC_1280 = lib.mkForce no;
      SCSI_SMARTPQI = lib.mkForce no;
      SCSI_SNIC = lib.mkForce no;
      SCSI_STEX = lib.mkForce no;
      SCSI_SYM53C8XX_2 = lib.mkForce no;
      SCSI_UFSHCD = lib.mkForce no;
      SCSI_WD719X = lib.mkForce no;

      # Keep NFS/FUSE; drop SMB/CIFS client and server support.
      CIFS = lib.mkForce no;
      SMB_SERVER = lib.mkForce no;
      SMBFS = lib.mkForce no;

      # Obsolete or niche buses/subsystems.
      CAN = lib.mkForce no;
      FIREWIRE = lib.mkForce no;
      FIREWIRE_NOSY = lib.mkForce no;
      HAMRADIO = lib.mkForce no;
      PCMCIA = lib.mkForce no;
      PCMCIA_XIRCOM = lib.mkForce no;
      USB_GADGET = lib.mkForce no;

      # TV, radio, capture-card, and media test drivers. Generic webcam
      # support is kept.
      DVB_CORE = lib.mkForce no;
      MEDIA_ANALOG_TV_SUPPORT = lib.mkForce no;
      MEDIA_DIGITAL_TV_SUPPORT = lib.mkForce no;
      MEDIA_RADIO_SUPPORT = lib.mkForce no;
      MEDIA_TEST_SUPPORT = lib.mkForce no;
      VIDEO_BT848 = lib.mkForce no;
      VIDEO_CX18 = lib.mkForce no;
      VIDEO_CX231XX = lib.mkForce no;
      VIDEO_CX23885 = lib.mkForce no;
      VIDEO_CX25821 = lib.mkForce no;
      VIDEO_CX88 = lib.mkForce no;
      VIDEO_EM28XX = lib.mkForce no;
      VIDEO_GO7007 = lib.mkForce no;
      VIDEO_IVTV = lib.mkForce no;
      VIDEO_PVRUSB2 = lib.mkForce no;
      VIDEO_SAA7134 = lib.mkForce no;
      VIDEO_SAA7164 = lib.mkForce no;
      VIDEO_SOLO6X10 = lib.mkForce no;
      VIDEO_STK1160 = lib.mkForce no;
      VIDEO_TW5864 = lib.mkForce no;
      VIDEO_TW68 = lib.mkForce no;
      VIDEO_TW686X = lib.mkForce no;
      VIDEO_VICODEC = lib.mkForce no;
      VIDEO_VIM2M = lib.mkForce no;
      VIDEO_VIMC = lib.mkForce no;
      VIDEO_VISL = lib.mkForce no;
      VIDEO_VIVID = lib.mkForce no;
      VIDEO_ZORAN = lib.mkForce no;
    });
  };

  customKernelPackages = baseKernelPackages.extend (_: _: {
    kernel = customKernel;
  });
in
{
  options.me.cachy-kernel = {
    enable = lib.mkEnableOption "the CachyOS zen4 kernel and matching system tweaks";

    amdgpu = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        When false, DRM_AMDGPU is compiled out and amdgpu is added to
        boot.blacklistedKernelModules. Set false on hosts where the AMD
        iGPU is intentionally unused behind a discrete NVIDIA GPU.
      '';
    };

    scheduler = lib.mkOption {
      type = lib.types.nullOr (lib.types.enum [ "scx_bpfland" ]);
      default = "scx_bpfland";
      description = ''
        sched-ext BPF scheduler to enable, or null to leave the default
        kernel scheduler in place.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelPackages = customKernelPackages;
    boot.zfs.package = config.boot.kernelPackages.zfs_cachyos;

    boot.kernelParams = [
      "nowatchdog"
      "zswap.enabled=1"
      "zswap.compressor=zstd"
      "zswap.max_pool_percent=20"
      "zswap.shrinker_enabled=1"
    ];

    boot.blacklistedKernelModules =
      [ "iTCO_wdt" "sp5100_tco" ]
      ++ lib.optional (! cfg.amdgpu) "amdgpu";

    boot.kernel.sysctl = {
      "vm.swappiness" = 10;
      "vm.vfs_cache_pressure" = 50;
      "vm.dirty_bytes" = 268435456;
      "vm.dirty_background_bytes" = 67108864;
      "vm.dirty_writeback_centisecs" = 1500;

      "kernel.nmi_watchdog" = 0;
      "kernel.printk" = "3 3 3 3";
      "kernel.kptr_restrict" = 2;
      "kernel.sysrq" = 128;
      "kernel.split_lock_mitigate" = 0;

      "fs.file-max" = 2097152;

      "net.core.netdev_max_backlog" = 4096;
      "net.ipv4.tcp_congestion_control" = "bbr";
      "net.core.default_qdisc" = "cake";
      "net.ipv4.tcp_fastopen" = 3;
      "net.core.rmem_max" = 16777216;
      "net.core.wmem_max" = 16777216;
      "net.core.somaxconn" = 8192;
      "net.ipv4.tcp_keepalive_time" = 60;
      "net.ipv4.tcp_keepalive_intvl" = 10;
      "net.ipv4.tcp_mtu_probing" = 1;
    };

    boot.kernel.sysfs = {
      kernel.mm.transparent_hugepage = {
        enabled = "always";
        defrag = "defer+madvise";
      };
    };

    services.udev.extraRules = ''
      ACTION=="add|change", KERNEL=="nvme[0-9]*", ATTR{queue/scheduler}="none"
      ACTION=="add|change", KERNEL=="sd[a-z]*|mmcblk[0-9]*", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="mq-deadline"
      ACTION=="add|change", KERNEL=="sd[a-z]*", ATTR{queue/rotational}=="1", ATTR{queue/scheduler}="bfq"
    '';

    systemd.settings.Manager = {
      DefaultTimeoutStartSec = "15s";
      DefaultTimeoutStopSec = "10s";
      DefaultLimitNOFILE = "2048:2097152";
    };

    services.journald.extraConfig = ''
      SystemMaxUse=1G
    '';

    boot.extraModprobeConfig = ''
      options nvidia NVreg_UsePageAttributeTable=1
      options nvidia NVreg_InitializeSystemMemoryAllocations=0
      options nvidia NVreg_DynamicPowerManagement=0x02
    '';

    services.fstrim.enable = true;

    security.pam.loginLimits = [
      { domain = "@audio"; type = "-"; item = "rtprio";  value = "95"; }
      { domain = "@audio"; type = "-"; item = "memlock"; value = "unlimited"; }
      { domain = "@audio"; type = "-"; item = "nice";    value = "-19"; }
    ];

    users.users.svein.extraGroups = [ "audio" ];

    services.ananicy = {
      enable = true;
      package = pkgs.ananicy-cpp;
      rulesProvider = pkgs.ananicy-cpp;
    };

    services.scx = lib.mkIf (cfg.scheduler != null) {
      enable = true;
      scheduler = cfg.scheduler;
    };
  };
}
