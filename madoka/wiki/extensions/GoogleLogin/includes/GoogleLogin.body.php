<?php

namespace GoogleLogin;

use ConfigFactory;

use Google_Client;

class GoogleLogin {
	/** @var $mGoogleClient Stores an instance of GoogleClient */
	private static $mGoogleClient;
	/** @var $mConfig Config object created for GoogleLogin extension */
	private static $mConfig;
	/** @var $mHost The Host of E-Mail provided by Google */
	private static $mHost;

	/**
	 * Returns an prepared instance of Google client to do requests with to Google API
	 * @return Google_Client
	 */
	public static function getClient( $returnToUrl, $token ) {
		if ( empty( self::$mGoogleClient ) ) {
			$glConfig = self::getGLConfig();
			$client = new Google_Client();
			$client->setClientId( $glConfig->get( 'GLAppId' ) );
			$client->setClientSecret( $glConfig->get( 'GLSecret' ) );
			$client->setRedirectUri( $returnToUrl );
			$client->addScope( 'profile' );
			$client->addScope( 'email' );
			$client->setState( $token );
			self::$mGoogleClient = $client;
		}
		return self::$mGoogleClient;
	}

	/**
	 * Returns Config object for use in GoogleLogin.
	 *
	 * @return Config
	 */
	public static function getGLConfig() {
		if ( self::$mConfig === null ) {
			self::$mConfig = ConfigFactory::getDefaultInstance()->makeConfig( 'googlelogin' );
		}
		return self::$mConfig;
	}

	/**
	 * If restriction of domains is enabled, check if the user E-Mail is valid before do anything.
	 * @param string $mailDomain The domain of email address
	 * @return boolean
	 */
	public static function isValidDomain( $mailDomain ) {
		$glConfig = self::getGLConfig();
		if ( is_array( $glConfig->get( 'GLAllowedDomains' ) ) ) {
			if (
				in_array(
					self::getHost( $mailDomain ),
					$glConfig->get( 'GLAllowedDomains' )
				)
			) {
				return true;
			}
			return false;
		}
		return true;
	}

	/**
	 * Returns the domain and tld (without subdomains) of the provided E-Mailadress
	 * @param string $domain The domain part of the email address to extract from.
	 * @return string The Tld and domain of $domain without subdomains
	 * @see http://www.programmierer-forum.de/domainnamen-ermitteln-t244185.htm
	 */
	public static function getHost( $domain = '' ) {
		$glConfig = self::getGLConfig();
		if ( !empty( self::$mHost ) ) {
			return self::$mHost;
		}
		$dir = __DIR__ . "/..";
		if ( $glConfig->get( 'GLAllowedDomainsStrict' ) ) {
			$domain = explode( '@', $domain );
			// we can trust google to give us only valid email address, so give the last element
			self::$mHost = array_pop( $domain );
			return self::$mHost;
		}
		// for parse_url()
		$domain =
			!isset( $domain[5] ) ||
			(
				$domain[3] != ':' &&
				$domain[4] != ':' &&
				$domain[5] != ':'
			) ? 'http://' . $domain : $domain;
		// remove "/path/file.html", "/:80", etc.
		$domain = parse_url( $domain, PHP_URL_HOST );
		// separate domain level
		$lvl = explode( '.', $domain ); // 0 => www, 1 => example, 2 => co, 3 => uk
		// set levels
		krsort( $lvl ); // 3 => uk, 2 => co, 1 => example, 0 => www
		$lvl = array_values( $lvl ); // 0 => uk, 1 => co, 2 => example, 3 => www
		$_1st = $lvl[0];
		$_2nd = isset( $lvl[1] ) ? $lvl[1] . '.' . $_1st : false;
		$_3rd = isset( $lvl[2] ) ? $lvl[2] . '.' . $_2nd : false;
		$_4th = isset( $lvl[3] ) ? $lvl[3] . '.' . $_3rd : false;

		// tld extract
		if ( !file_exists( "$dir/cache/tld.txt" ) ) {
			self::createTLDCache( "$dir/cache/tld.txt" );
		}
		require ( "$dir/cache/tld.txt" );
		$tlds = array_flip( $tlds );
		if ( // fourth level is TLD
			$_4th &&
			!isset( $tlds[ '!' . $_4th ] ) &&
			(
				isset( $tlds[ $_4th ] ) ||
				isset( $tlds[ '*.' . $_3rd ] )
			)
		) {
			$domain = isset( $lvl[4] ) ? $lvl[4] . '.' . $_4th : false;
		} elseif ( // third level is TLD
			$_3rd &&
			!isset( $tlds[ '!' . $_3rd ] ) &&
			(
				isset( $tlds[ $_3rd ] ) ||
				isset( $tlds[ '*.' . $_2nd ] )
			)
		) {
			$domain = $_4th;
		} elseif ( // second level is TLD
			!isset( $tlds[ '!' . $_2nd ] ) &&
			(
				isset( $tlds[ $_2nd ] ) ||
				isset( $tlds[ '*.' . $_1st ] )
			)
		) {
			$domain = $_3rd;
		} else { // first level is TLD
			$domain = $_2nd;
		}
		self::$mHost = $domain;
		return self::$mHost;
	}

	/**
	 * Creates the TLD cache from which the valid tld of mail domain comes from.
	 * @param string $cacheFile The file to create the cache too (must be writeable for the
	 * webserver!)
	 * @param int $max_tl How deep the domain list is (enclude example.co.uk (2) or
	 * example.lib.wy.us (3)?)
	 * @see http://www.programmierer-forum.de/domainnamen-ermitteln-t244185.htm
	 */
	public static function createTLDCache( $cacheFile, $max_tl = 2 ) {
		$cacheFolder = str_replace( basename( $cacheFile ), '', $cacheFile );
		if ( !is_writable( $cacheFolder ) ) {
			throw new MWException( $cacheFolder . ' is not writeable!' );
		}
		$tlds = file(
			'http://mxr.mozilla.org/mozilla-central/source/netwerk/dns/effective_tld_names.dat?raw=1'
		);
		if ( $tlds === false ) {
			throw new MWException( 'Domainlist can not be downloaded!' );
		}
		$i = 0;
		// remove unnecessary lines
		foreach ( $tlds as $tld ) {
			$tlds[ $i ] = trim( $tld );
			/**
			 *	empty
			 *	comments
			 *	top level domains
			 *	is overboard
			*/
			if (
				!$tlds[ $i ] ||
				$tld[0] == '/' ||
				strpos( $tld, '.' ) === false ||
				substr_count( $tld, '.' ) >= $max_tl
			) {
				unset( $tlds[ $i ] );
			}
			$i++;
		}
		$tlds = array_values( $tlds );
		file_put_contents(
			$cacheFile,
			"<?php\n" . '$tlds = ' . str_replace(
				[ ' ', "\n" ],
				'',
				var_export( $tlds, true )
			) . ";\n?" . ">"
		);
	}
}
