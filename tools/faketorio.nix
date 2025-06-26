{ argparse, buildLuarocksPackage, fetchFromGitHub, fetchurl, lua-zlib, luafilesystem, luarocks, luaOlder }:

let
  json-lua =
    buildLuarocksPackage {
      pname = "json-lua";
      version = "0.1-4";
      knownRockspec = (fetchurl {
        url = "mirror://luarocks/json-lua-0.1-4.rockspec";
        sha256 = "11p70k2c3rxpnzm3901720cp4pz9mnhm8kvkhnwdcdh0v7if8yxv";
      }).outPath;
      src = fetchFromGitHub {
        owner = "tiye";
        repo = "json-lua";
        rev = "e20272b079d7a64e06cf566f7f1e4aecdc369cd0";
        hash = "sha256-5lAHJGYM86xCChQvUCJ+eSKO6FD2lEgrf096A+1iCYA=";
      };

      disabled = luaOlder "5.1";

      meta = {
        homepage = "https://github.com/tiye/json-lua";
        description = "JSON encoder/decoder";
        license.fullName = "CC";
      };
    };
in

buildLuarocksPackage {
  pname = "faketorio";
  version = "1.6.0-1";
  knownRockspec = (fetchurl {
    url = "mirror://luarocks/faketorio-1.6.0-1.rockspec";
    sha256 = "00y1k70j6a49w1xvw0z84whn9fsb7k7vs2cwwqym408vv4616mkz";
  }).outPath;
  src = fetchFromGitHub {
    owner = "JonasJurczok";
    repo = "Faketorio";
    rev = "1.6.0";
    hash = "sha256-oK5H8DdA+81SdRkDHu+i1FDvCh8XBMCyjE5MtzXYat0=";
  };

  propagatedBuildInputs = [ argparse json-lua lua-zlib luafilesystem luarocks ];

  meta = {
    homepage = "http://github.com/JonasJurczok/Faketorio";
    description = "Support library for Factorio mod unit testing.";
    license.fullName = "MIT/X11";
  };
}

