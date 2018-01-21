<?php
/**
 * Special:GoogleLogin implementation
 */

namespace GoogleLogin\Specials;

use Exception;

use GoogleLogin\GoogleLogin;
use GoogleLogin\GoogleUser as User;

use Google_Client;
use Google_Service_Plus;
use SpecialPage;

/**
 * Implements a special page that gives information about a specific Google user, after
 * this Google user authenticates itself and permitted that this application can read his
 * user data saved by Google.
 */
class SpecialGoogleLogin extends \SpecialPage {
	function __construct() {
		parent::__construct( 'GoogleLogin' );
	}

	function execute( $par ) {
		$request = $this->getRequest();
		$out = $this->getOutput();
		$client = GoogleLogin::getClient( $this->getTitle()->getFullURL(), null );

		$this->setHeaders();

		// every time enable OOUI on this special page
		$out->enableOOUI();

		// add module styles
		$out->addModules( 'ext.GoogleLogin.style' );

		// it's possible, that the session isn't started yet (if GoogleLogin
		// replaces MediaWiki login, e.g.)
		$this->getRequest()->getSession()->persist();

		// initialize the client for google plus api
		$plus = new Google_Service_Plus( $client );

		// if the user is redirected back from google, try to authenticate
		$authCode = $request->getVal( 'code' );
		if ( $authCode !== null ) {
			$this->tryAuthenticate( $authCode, $client, $plus );
		} elseif ( $request->getVal( 'error' ) !== null ) {
			// if there was an error reported from google, show this to the user
			// FIXME: This should be a localized message!
			$this->createError( 'Authentication failed' );
		} else {
			$access_token = $request->getSessionData( 'access_token' );
			if ( $access_token !== null ) {
				$client->setAccessToken( $access_token );
				$request->setSessionData( 'access_token', $client->getAccessToken() );

				$this->showSummary( $plus );
			} else {
				$authUrl = $client->createAuthUrl();
				$out->redirect( $authUrl );
			}
		}
	}

	/**
	 * Tries to get the user information of the passed plus object and
	 * fails savely by adding an error message, if an Exception occurs.
	 *
	 * @param Google_Service_Plus $plus
	 * @return bool|array
	 */
	private function getPlusUserInfo( Google_Service_Plus $plus ) {
		try {
			return $userInfo = $plus->people->get( "me" );
		} catch ( Exception $e ) {
			$this->createError( $e->getMessage() );
			return false;
		}
	}

	/**
	 * Helper function to authenticate a user against google plus api
	 *
	 * @param String $code The auth code to use
	 * @param Google_Client $client
	 * @param Google_Service_Plus $plus
	 */
	private function tryAuthenticate( $authCode, Google_Client &$client, Google_Service_Plus &$plus ) {
		$request = $this->getRequest();
		try {
			$client->authenticate( $authCode );
			$request->setSessionData( 'access_token', $client->getAccessToken() );
			$userInfo = $this->getPlusUserInfo( $plus );
			if ( $userInfo ) {
				$this->showSummary( $plus );
			}
		} catch ( Exception $e ) {
			$this->createError( $e->getMessage() );
		}
	}

	/**
	 * Show a summary about the actual logged in google user.
	 *
	 * @param Google_Service_Plus $plus
	 */
	private function showSummary( Google_Service_Plus $plus ) {
		$out = $this->getOutput();

		$userInfo = $this->getPlusUserInfo( $plus );
		if ( !$userInfo ) {
			return;
		}

		$isLinked = !User::isGoogleIdFree( $userInfo['id'] );
		// data that will be added to the account information box
		$data = [
			'Google-ID' => $userInfo['id'],
			$this->msg( 'googlelogin-googleuser' )->text() => $userInfo['displayName'],
			$this->msg( 'googlelogin-email' )->text() => $userInfo['emails'][0]['value'],
			$this->msg( 'googlelogin-linkstatus' )->text() => ( $isLinked ?
				$this->msg( 'googlelogin-linked' )->text() : $this->msg( 'googlelogin-unlinked' )->text() ),
		];

		$items = [];
		// expand the data to ooui elements
		foreach ( $data as $label => $d ) {
			$items[] = new \OOUI\FieldLayout(
				new \OOUI\LabelWidget( [
					'label' => $d
				] ),
				[
					'align' => 'left',
					'label' => $label
				]
			);
		}

		// create a wrapper panel
		$container = new \OOUI\PanelLayout( [
			'padded' => true,
			'expanded' => false,
			'framed' => true,
		] );

		// add the fieldset to the wrapper panel and output it
		$container->appendContent(
			new \OOUI\FieldsetLayout( [
				'label' => $this->msg( 'googlelogin-information-title' )->text(),
				'items' => $items,
			] )
		);

		$out->addHTML( $container );
	}

	/**
	 * Creates a generic error message with further information in $errorMessage.
	 * @param string $errorMessage short description or further information to the error
	 */
	private function createError( $errorMessage ) {
		$out = $this->getOutput();
		$out->addWikiMsg( 'googlelogin-generic-error', $errorMessage );
	}
}
