{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "nix-check-cached";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    openssl
  ] ++ lib.optionals stdenv.isDarwin [
    darwin.apple_sdk.frameworks.Security
    darwin.apple_sdk.frameworks.SystemConfiguration
  ];

  meta = with pkgs.lib; {
    description = "Cached nix flake check using Redis and Jujutsu";
    homepage = "https://github.com/sveina/nixos";
    license = licenses.mit;
    maintainers = [ ];
    platforms = platforms.linux;
  };
}
