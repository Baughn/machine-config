{
  # WINE CPU topology for AMD 7950X3D to prefer V-Cache cores
  environment.sessionVariables = {
    # Cores 0-7 and their SMT siblings 16-23 are on the V-Cache CCD
    WINE_CPU_TOPOLOGY = "16:0,1,2,3,4,5,6,7,16,17,18,19,20,21,22,23";
  };
}
