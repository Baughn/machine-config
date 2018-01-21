<?php
/**
 * GooglePrimaryAuthenticationProvider implementation
 */

namespace GoogleLogin\Auth;

use Exception;

use MediaWiki\Auth\AuthenticationRequest;
use MediaWiki\Auth\AbstractPrimaryAuthenticationProvider;
use MediaWiki\Auth\AuthManager;
use MediaWiki\Auth\AuthenticationResponse;
use User;

use GoogleLogin\GoogleUser;
use GoogleLogin\GoogleLogin;

use Google_Service_Plus;

use StatusValue;
use SpecialPage;

/**
 * Implements a primary authentication provider to authenticate an user using a Google account where
 * this user has access, too. On beginning of the authentication, the provider maybe redirects the
 * user to an external authentication provider (Google) to authenticate and permit the access to
 * the data of the foreign account, before it actually authenticates the user.
 */
class GooglePrimaryAuthenticationProvider extends AbstractPrimaryAuthenticationProvider {
	/** Session inside of the auth session data where the original redirect URL is saved */
	const RETURNURL_SESSION_KEY = 'googleLoginReturnToUrl';
	/** Token salt for CSRF token used by GoogleLogin when a user gets
	 * redirected from Google */
	const TOKEN_SALT = 'GooglePrimaryAuthenticationProvider:redirect';
	/** Name of the button of the GoogleAuthenticationRequest */
	const GOOGLELOGIN_BUTTONREQUEST_NAME = 'googlelogin';

	public function beginPrimaryAuthentication( array $reqs ) {
		return $this->beginGoogleAuthentication( $reqs, self::GOOGLELOGIN_BUTTONREQUEST_NAME );
	}

	public function continuePrimaryAuthentication( array $reqs ) {
		$request = AuthenticationRequest::getRequestByClass( $reqs,
			GoogleServerAuthenticationRequest::class );
		if ( !$request ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-error-no-authentication-workflow' )
			);
		}
		$plus = $this->getAuthenticatedGooglePlusFromRequest( $request );
		if ( $plus instanceof AuthenticationResponse ) {
			return $plus;
		}

		try {
			$userInfo = $plus->people->get( "me" );
			$user = GoogleUser::getUserFromGoogleId( $userInfo['id'] );
			if ( $user ) {
				if ( !GoogleLogin::isValidDomain( $userInfo['emails'][0]['value'] ) ) {
					return AuthenticationResponse::newFail(
						wfMessage( 'googlelogin-unallowed-domain', GoogleLogin::getHost() )
					);
				}
				return AuthenticationResponse::newPass( $user->getName() );
			} else {
				$resp = AuthenticationResponse::newPass( null );
				$resp->linkRequest = new GoogleUserInfoAuthenticationRequest( $userInfo );
				$resp->createRequest = $resp->linkRequest;
				return $resp;
			}
		} catch ( Exception $e ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-generic-error', $e->getMessage() )
			);
		}
	}

	public function getAuthenticationRequests( $action, array $options ) {
		switch ( $action ) {
			case AuthManager::ACTION_LOGIN:
				return [ new GoogleAuthenticationRequest(
					wfMessage( 'googlelogin' ),
					wfMessage( 'googlelogin-loginbutton-help' )
				) ];
				break;
			case AuthManager::ACTION_LINK:
				// TODO: Probably not the best message currently.
				return [ new GoogleAuthenticationRequest(
					wfMessage( 'googlelogin-form-merge' ),
					wfMessage( 'googlelogin-link-help' )
				) ];
				break;
			case AuthManager::ACTION_REMOVE:
				$user = User::newFromName( $options['username'] );
				if ( !$user || !GoogleUser::hasConnectedGoogleAccount( $user ) ) {
					return [];
				}
				$googleIds = Googleuser::getGoogleIdFromUser( $user );
				$reqs = [];
				foreach ( $googleIds as $key => $id ) {
					$reqs[] = new GoogleRemoveAuthenticationRequest( $id );
				}
				return $reqs;
				break;
			case AuthManager::ACTION_CREATE:
				// TODO: ACTION_CREATE doesn't really need all
				// the things provided by inheriting
				// ButtonAuthenticationRequest, so probably it's better
				// to create it's own Request
				return [ new GoogleAuthenticationRequest(
					wfMessage( 'googlelogin-create' ),
					wfMessage( 'googlelogin-link-help' )
				) ];
				break;
			default:
				return [];
		}
	}

	public function testUserExists( $username, $flags = User::READ_NORMAL ) {
		return false;
	}

	public function testUserCanAuthenticate( $username ) {
		$user = \User::newFromName( $username );
		if ( $user ) {
			return GoogleUser::hasConnectedGoogleAccount( $user );
		}
		return false;
	}

	public function providerAllowsAuthenticationDataChange(
		AuthenticationRequest $req, $checkData = true
	) {
		if (
			get_class( $req ) === GoogleRemoveAuthenticationRequest::class &&
			$req->action === AuthManager::ACTION_REMOVE
		) {
			$user = User::newFromName( $req->username );
			if ( $user && in_array( $req->getGoogleId(), GoogleUser::getGoogleIdFromUser( $user ) ) ) {
				return StatusValue::newGood();
			} else {
				return StatusValue::newFatal( wfMessage( 'googlelogin-change-account-not-linked' ) );
			}
		}

		if (
			get_class( $req ) === GoogleUserInfoAuthenticationRequest::class &&
			$req->action === AuthManager::ACTION_CHANGE
		) {
			$user = User::newFromName( $req->username );
			$potentialUser = GoogleUser::getUserFromGoogleId( $req->userInfo['id'] );
			if ( $potentialUser && !$potentialUser->equals( $user ) ) {
				return StatusValue::newFatal( 'googlelogin-link-other1' );
			} elseif ( $potentialUser ) {
				return StatusValue::newFatal( 'googlelogin-link-same' );
			}
			if ( $user ) {
				return StatusValue::newGood();
			}
		}
		return StatusValue::newGood( 'ignored' );
	}

	public function providerChangeAuthenticationData( AuthenticationRequest $req ) {
		if (
			get_class( $req ) === GoogleRemoveAuthenticationRequest::class &&
			$req->action === AuthManager::ACTION_REMOVE
		) {
			$user = User::newFromName( $req->username );
			GoogleUser::terminateGoogleConnection( $user, $req->getGoogleId() );
		}

		if (
			get_class( $req ) === GoogleUserInfoAuthenticationRequest::class &&
			$req->action === AuthManager::ACTION_CHANGE
		) {
			$user = User::newFromName( $req->username );
			GoogleUser::connectWithGoogle( $user, $req->userInfo['id'] );
		}
	}

	public function providerNormalizeUsername( $username ) {
		return null;
	}

	public function accountCreationType() {
		return self::TYPE_LINK;
	}

	public function beginPrimaryAccountCreation( $user, $creator, array $reqs ) {
		$request = AuthenticationRequest::getRequestByClass( $reqs,
			GoogleUserInfoAuthenticationRequest::class );
		if ( $request ) {
			if ( GoogleUser::isGoogleIdFree( $request->userInfo['id'] ) ) {
				$resp = AuthenticationResponse::newPass();
				$resp->linkRequest = $request;
				return $resp;
			}
		}
		return $this->beginGoogleAuthentication( $reqs, self::GOOGLELOGIN_BUTTONREQUEST_NAME );
	}

	public function continuePrimaryAccountCreation( $user, $creator, array $reqs ) {
		$request = AuthenticationRequest::getRequestByClass( $reqs,
			GoogleServerAuthenticationRequest::class );
		if ( !$request ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-error-no-authentication-workflow' )
			);
		}
		$plus = $this->getAuthenticatedGooglePlusFromRequest( $request );
		if ( $plus instanceof AuthenticationResponse ) {
			return $plus;
		}
		try {
			$userInfo = $plus->people->get( "me" );
			$isGoogleIdFree = GoogleUser::isGoogleIdFree( $userInfo['id'] );
			if ( $isGoogleIdFree ) {
				if ( !GoogleLogin::isValidDomain( $userInfo['emails'][0]['value'] ) ) {
					return AuthenticationResponse::newFail(
						wfMessage( 'googlelogin-unallowed-domain', GoogleLogin::getHost() )
					);
				}
				$resp = AuthenticationResponse::newPass();
				$resp->linkRequest = new GoogleUserInfoAuthenticationRequest( $userInfo );
				return $resp;
			}
			return AuthenticationResponse::newFail( wfMessage( 'googlelogin-link-other' ) );
		} catch ( Exception $e ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-generic-error', $e->getMessage() )
			);
		}
	}

	public function finishAccountCreation( $user, $creator, AuthenticationResponse $response ) {
		$userInfo = $response->linkRequest->userInfo;
		$user->setEmail( $userInfo['emails'][0]['value'] );
		$user->saveSettings();
		GoogleUser::connectWithGoogle( $user, $userInfo['id'] );

		return null;
	}

	public function beginPrimaryAccountLink( $user, array $reqs ) {
		return $this->beginGoogleAuthentication( $reqs, self::GOOGLELOGIN_BUTTONREQUEST_NAME );
	}

	public function continuePrimaryAccountLink( $user, array $reqs ) {
		$request = AuthenticationRequest::getRequestByClass( $reqs,
			GoogleServerAuthenticationRequest::class );
		if ( !$request ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-error-no-authentication-workflow' )
			);
		}
		$client = $this->getGoogleClient();
		$client->authenticate( $request->accessToken );
		$plus = new Google_Service_Plus( $client );
		try {
			$userInfo = $plus->people->get( "me" );
			$googleId = $userInfo['id'];
			$potentialUser = GoogleUser::getUserFromGoogleId( $googleId );
			if ( $potentialUser && !$potentialUser->equals( $user ) ) {
				return AuthenticationResponse::newFail( wfMessage( 'googlelogin-link-other' ) );
			} elseif ( $potentialUser ) {
				return AuthenticationResponse::newFail( wfMessage( 'googlelogin-link-same' ) );
			} else {
				if ( !GoogleLogin::isValidDomain( $userInfo['emails'][0]['value'] ) ) {
					return AuthenticationResponse::newFail(
						wfMessage( 'googlelogin-unallowed-domain', GoogleLogin::getHost() )
					);
				}
				$result = GoogleUser::connectWithGoogle( $user, $googleId );
				if ( $result ) {
					return AuthenticationResponse::newPass();
				} else {
					// TODO: Better error message
					return AuthenticationResponse::newFail( new \RawMessage( 'Database error' ) );
				}
			}
		} catch ( Exception $e ) {
			return AuthenticationResponse::newFail(
				wfMessage( 'googlelogin-generic-error', $e->getMessage() )
			);
		}
	}

	/**
	 * Handler for a primary authentication, which currently begins. Checks, if the Authentication
	 * request can be handled by GoogleLogin and, if so, returns an AuthenticationResponse that
	 * redirects to the external authentication site of Google, otherwise returns an abstain response.
	 * @param array $reqs
	 * @param $buttonAuthenticationRequestName
	 * @return AuthenticationResponse
	 */
	private function beginGoogleAuthentication( array $reqs, $buttonAuthenticationRequestName ) {
		$req = GoogleAuthenticationRequest::getRequestByName( $reqs, $buttonAuthenticationRequestName );
		if ( !$req ) {
			return AuthenticationResponse::newAbstain();
		}
		$client = $this->getGoogleClient();
		$this->manager->setAuthenticationSessionData( self::RETURNURL_SESSION_KEY, $req->returnToUrl );

		return AuthenticationResponse::newRedirect( [
			new GoogleServerAuthenticationRequest()
		], $client->createAuthUrl() );
	}

	/**
	 * Returns an instance of Google_Client, which is set up for the use in an authentication workflow.
	 *
	 * @return \Google_Client
	 */
	public function getGoogleClient() {
		$client = GoogleLogin::getClient(
			SpecialPage::getTitleFor( 'GoogleLoginReturn' )->getFullURL(),
			$this->manager->getRequest()->getSession()->getToken( self::TOKEN_SALT )->toString()
		);

		return $client;
	}

	/**
	 * Creates a new authenticated Google Plus Service from a GoogleServerAuthenticationRequest.
	 *
	 * @param $request
	 * @return Google_Service_Plus|AuthenticationResponse
	 */
	private function getAuthenticatedGooglePlusFromRequest( GoogleServerAuthenticationRequest
		$request
	) {
		if ( !$request->accessToken || $request->errorCode ) {
			switch ( $request->errorCode ) {
				case 'access_denied':
					return AuthenticationResponse::newFail( wfMessage( 'googlelogin-access-denied'
						) );
					break;
				default:
					return AuthenticationResponse::newFail( wfMessage(
						'googlelogin-generic-error', $request->errorCode ? $request->errorCode :
						'unknown' ) );
			}
		}
		$client = $this->getGoogleClient();
		$client->authenticate( $request->accessToken );
		$plus = new Google_Service_Plus( $client );

		return $plus;
	}
}
