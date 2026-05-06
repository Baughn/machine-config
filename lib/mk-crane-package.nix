{ craneLib, lib }:

args:
let
  packageArgs = builtins.removeAttrs args [ "extraFiles" ];
in
craneLib.buildPackage (packageArgs // {
  src =
    if args ? extraFiles then
      lib.fileset.toSource {
        root = args.src;
        fileset = lib.fileset.unions ([ (craneLib.fileset.commonCargoSources args.src) ] ++ args.extraFiles);
      }
    else
      craneLib.cleanCargoSource args.src;
  strictDeps = args.strictDeps or true;
})
