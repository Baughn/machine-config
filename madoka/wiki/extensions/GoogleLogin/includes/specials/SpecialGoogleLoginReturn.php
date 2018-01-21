<?php
/**
 * SpecialGoogleLoginReturn implementation
 */

namespace GoogleLogin\Specials;

use GoogleLogin\Auth\GooglePrimaryAuthenticationProvider;
use SpecialPage;
use UnlistedSpecialPage;

/**
 * Helper special page for constant redirect target required for Google authentication.
 * Redirects to an already started authentication workflow with the appropriate data).
 */
class SpecialGoogleLoginReturn extends UnlistedSpecialPage {
	function __construct() {
		parent::__construct( 'GoogleLoginReturn' );
	}

	/**
	 * Special page executer
	 * @param string $par Subpage
	 */
	function execute( $par ) {
		$request = $this->getRequest();
		$session = $request->getSession();
		$out = $this->getOutput();
		$this->setHeaders();
		$authData = $session->getSecret( 'authData' );
		$token = $session->getToken( GooglePrimaryAuthenticationProvider::TOKEN_SALT );
		$redirectUrl = $authData[GooglePrimaryAuthenticationProvider::RETURNURL_SESSION_KEY];
		if ( !$redirectUrl || !$token->match( $request->getVal( 'state' ) ) ) {
			$out->redirect( SpecialPage::getTitleFor( 'UserLogin' )->getLocalURL() );
			return;
		}
		$code = $request->getVal( 'code' );
		if ( $code ) {
			$redirectUrl = wfAppendQuery( $redirectUrl, [ 'code' => $code ] );
		}

		$error = $request->getVal( 'error' );
		if ( $error ) {
			$redirectUrl = wfAppendQuery( $redirectUrl, [ 'error' => $error ] );
		}
		$out->redirect( $redirectUrl );
	}
}
