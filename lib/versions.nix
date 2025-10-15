{ lib }:

{
  # Select the derivation with the newest version from a list of derivations
  #
  # Uses builtins.compareVersions to compare version strings.
  #
  # Type: selectNewest :: [Derivation] -> Derivation
  #
  # Example:
  #   selectNewest [
  #     pkgs.vintagestory
  #     (pkgs.vintagestory.overrideAttrs (old: {
  #       version = "1.22.0";
  #       src = ...;
  #     }))
  #   ]
  #   => returns the derivation with version "1.22.0"
  #
  # AIDEV-NOTE: Uses builtins.compareVersions which returns:
  #   1 if first version is newer
  #  -1 if first version is older
  #   0 if versions are equal
  selectNewest = drvs:
    let
      # Helper to safely get version from a derivation
      getVersion = drv:
        if drv ? version then drv.version
        else if drv ? name then
        # Try to extract version from name (e.g., "package-1.2.3" -> "1.2.3")
          let
            parts = lib.splitString "-" drv.name;
            lastPart = lib.last parts;
          in
          # Check if last part looks like a version (starts with a digit)
          if builtins.match "[0-9].*" lastPart != null then lastPart
          else throw "Derivation ${drv.name} has no version attribute and name doesn't contain a version"
        else throw "Cannot determine version for derivation";

      # Fold over the list to find the derivation with the highest version
      result = lib.foldl
        (acc: drv:
          if acc == null then drv
          else
            let
              accVersion = getVersion acc;
              drvVersion = getVersion drv;
              comparison = builtins.compareVersions drvVersion accVersion;
            in
            if comparison > 0 then drv else acc
        )
        null
        drvs;
    in
    if drvs == [ ] then
      throw "selectNewest: empty list provided"
    else if result == null then
      throw "selectNewest: internal error - result should not be null"
    else
      result;
}
