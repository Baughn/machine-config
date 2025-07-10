{
  buildGoModule,
  fetchFromGitHub,
}:
buildGoModule rec {
  pname = "alertmanager-discord";
  version = "0.1.0";

  #  src = fetchFromGitHub {
  #    owner = "benjojo";
  #    repo = pname;
  #    rev = "3b8af1f97075aa7fb00d6b090e36166f6b86ca87";
  #    sha256 = "eVxPDQSzUQQ8fIRq3N4F9l4mYM86zSEOrgdHVv25S64=";
  #  };
  src = ./src;

  vendorHash = null;
}
