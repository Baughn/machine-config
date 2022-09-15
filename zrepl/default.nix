{
  stdenv,
  buildGoModule,
  fetchFromGitHub,
}:
buildGoModule rec {
  name = "zrepl-unstable-${version}";
  version = "0.2.1";

  src = fetchFromGitHub {
    owner = "zrepl";
    repo = "zrepl";
    rev = "v${version}";
    sha256 = "1y5k0ym128a7liqbmg8ywclp5w1wsds8dilhhd780hd6rk78d9yg";
  };

  vendorSha256 = "02ka7wymzky8hh4vqlfzyfq0kajy3320y2jmi013jzp0qynww2py";

  subPackages = ["."];

  # TODO: add metadata https://nixos.org/nixpkgs/manual/#sec-standard-meta-attributes
  meta = {
  };
}
