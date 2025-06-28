let
  svein = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDFqQOHIaerfzhi0pQHZ/U1ES2yvql9NY46A01TjmgAl" # Tsugumi
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINWui+bKBClX6dmReodwiboUKLoGX7MpnITB3UZR1Zma" # MBA
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPtwB25Bfq/fJgP0tnVwT1oPfWfZ2zixGhYH/KElG2EH" # tromso
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIbYhNmekOUSd3M84+jxwo7Q7B7mVsxoJhZc3v94n9Sj" # saya
  ];
  users = svein;

  kaho = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBSt1vGHa7g8qgXyuvtMphl8kvLpIoKG0bODkAdJe+10";
  saya = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBelCbc1HV39zkfebVfoxfj8SSnAEN5vyL8hRj1QF9p6";
  tromso = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBJHlcM3bzMi8xQAsRIrOBP9fDTQ/5yaPqaVFVcDKn9z";
  tsugumi = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBQ/0hKkb/+12T9ZzQ0lvu13JEL0RZJMxZ27WaQw9+3K";
  v4 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICn/KDuz5Ie7wJx/s+8hGYur/vMuYoyv6ZkbA+y+cONa";
  systems = [kaho saya tromso tsugumi v4];

  all = users ++ systems;
  host = h: [h] ++ svein;
in {
  # Web stuff
  "caddy.env.age".publicKeys = host tsugumi;
}
