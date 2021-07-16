{ buildGoModule, fetchFromGitHub }:

buildGoModule rec {
  pname = "alertamanager-discord";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "benjojo";
    repo = pname;
    rev = "3b8af1f97075aa7fb00d6b090e36166f6b86ca87";
    sha256 = "0m2fzpqxk7hrbxsgqplkg7h2p7gv6s1miymv3gvw0cz039skag2s";
  };

  vendorSha256 = "0m2fzpqxk7hrbxsgqplkg7h2p7gv6s1miymv3gvw0cz039skag1s";

  runVend = true;
}
