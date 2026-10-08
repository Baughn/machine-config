# Blue Prince stream (~/dev/blueprince/headless): MediaMTX LL-HLS on 8888 and the viewer page on 8890,
# both run as user units by headless/start. Public via tsugumi's caddy at bp.brage.info.
{
  networking.firewall.allowedTCPPorts = [ 8888 8890 ];
}
