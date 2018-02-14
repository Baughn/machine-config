{ config, pkgs, ... }:

{
  services.kubernetes = {
    roles = [ "master" "node" ];
    addons.dashboard.enable = true;
    apiserver.securePort = 444;
    apiserver.basicAuthFile = ../secrets/kubernetes-tokens.csv;
    # apiserver.serviceAccountKeyFile = ../secrets/kubernetes/serviceAccount.key;
    apiserver.clientCaFile = ../secrets/kubernetes/ca.crt;
  };
  environment.systemPackages = with pkgs; [
    kubernetes kubernetes-helm
  ];
}
