# Use the Zen 4 (znver4) optimized CachyOS kernel variant.
#
# The xddxdd/nix-cachyos-kernel flake provides pre-built kernel variants
# targeting specific architectures. This selects the zen4 variant, which
# is compiled with -march=znver4 enabling AVX-512, VNNI, and other
# Zen 4 specific instructions.
#
# Only the kernel is recompiled for znver4. Userspace packages use the
# standard binary cache. For per-package znver4 optimization, add
# individual packages to the overlay below.

{ lib, pkgs, ... }:

let
  baseKernelPackages = pkgs.cachyosKernels.linuxPackages-cachyos-latest-zen4;
  kernel = baseKernelPackages.kernel.override {
    structuredExtraConfig = baseKernelPackages.kernel.structuredExtraConfig // (with lib.kernel; {
      # Compile out AF_ALG, the userspace socket API for the kernel crypto
      # subsystem. CVE-2026-31431 is in algif_aead; the rest of the family is
      # disabled with it because nothing on saya depends on AF_ALG.
      CRYPTO_USER_API = lib.mkForce no;
      CRYPTO_USER_API_AEAD = lib.mkForce no;
      CRYPTO_USER_API_HASH = lib.mkForce no;
      CRYPTO_USER_API_RNG = lib.mkForce no;
      CRYPTO_USER_API_SKCIPHER = lib.mkForce no;

      # saya has an NVIDIA dGPU and the Raphael iGPU is intentionally unused
      # and blacklisted.
      DRM_AMDGPU = lib.mkForce no;
      DRM_I915 = lib.mkForce no;
      DRM_XE = lib.mkForce no;
      DRM_NOUVEAU = lib.mkForce no;
      DRM_RADEON = lib.mkForce no;

      # No local AI accelerator cards.
      DRM_ACCEL_AMDXDNA = lib.mkForce no;
      DRM_ACCEL_HABANALABS = lib.mkForce no;
      DRM_ACCEL_IVPU = lib.mkForce no;
      DRM_ACCEL_QAIC = lib.mkForce no;

      # Obsolete and virtual display drivers. Keep simpledrm/EFI framebuffer
      # for early boot and the proprietary NVIDIA kernel module path.
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

      # Embedded panels/bridges are irrelevant for a desktop with external
      # monitors on the NVIDIA card.
      DRM_PANEL = lib.mkForce no;
      DRM_BRIDGE = lib.mkForce no;

      # Local PCIe NVMe is used; remote NVMe fabrics and target mode are not.
      NVME_AUTH = lib.mkForce no;
      NVME_FABRICS = lib.mkForce no;
      NVME_FC = lib.mkForce no;
      NVME_HOST_AUTH = lib.mkForce no;
      NVME_KEYRING = lib.mkForce no;
      NVME_RDMA = lib.mkForce no;
      NVME_TCP = lib.mkForce no;
      NVME_TARGET = lib.mkForce no;

      # Keep SCSI core/libata for SATA and USB mass storage. Drop old and
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

      # TV, radio, capture-card, and media test drivers. Keep generic webcam
      # support rather than removing the whole media core.
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
in
{
  boot.kernelPackages = lib.mkForce (pkgs.linuxPackagesFor kernel);
}
