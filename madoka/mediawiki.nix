{ config, pkgs, lib, ... }:

{
  containers.eln-wiki = {
    autoStart = true;
    privateNetwork = true;
    hostAddress = "192.168.10.1";
    localAddress = "192.168.10.2";
    config =
      { config, pkgs, ... }:
      with pkgs.lib;
      {
        services.httpd.enable = true;
        services.httpd.adminAddr = "sveina@gmail.com";
        services.httpd.extraSubservices = singleton {
          serviceType = "mediawiki";
          id = "eln";
          siteName = "Electrical Wiki";
          logo = "https://electrical-age.net/assets/elnlogocube.png";
          enableUploads = true;
	  uploadDir = "/images/";
          defaultSkin = "vector";
          extraConfig = ''
	    $wgServer = "https://wiki.brage.info/";
	  
            wfLoadSkin( 'CologneBlue' );
            wfLoadSkin( 'Modern' );
            wfLoadSkin( 'MonoBook' );
            wfLoadSkin( 'Vector' );

            wfLoadExtension( 'CategoryTree' );
            require_once "$IP/extensions/ConfirmAccount/ConfirmAccount.php";
            wfLoadExtension( 'UserMerge' );
            $wgGroupPermissions['bureaucrat']['usermerge'] = true;
            require_once "$IP/extensions/SimpleMathJax/SimpleMathJax.php";
            wfLoadExtension( 'WikiEditor' );

            $wgGroupPermissions['*']['edit'] = false;
            $wgGroupPermissions['*']['createaccount'] = false;
          '';
          extensions = [ ./wiki/extensions ];
        };
        services.postgresql.enable = true;
        services.postgresql.authentication = ''
          local all all trust
        '';
        networking.firewall.allowedTCPPorts = [ 80 ];
        environment.systemPackages = [ pkgs.postgresql ];
      };
  };
}
