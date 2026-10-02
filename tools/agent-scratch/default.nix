{ writeShellApplication, coreutils, findutils }:
writeShellApplication {
  name = "scratch";
  runtimeInputs = [ coreutils findutils ];
  text = builtins.readFile ./scratch.sh;
}
