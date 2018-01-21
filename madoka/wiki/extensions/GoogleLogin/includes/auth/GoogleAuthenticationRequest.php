<?php
/**
 * GoogleAuthenticationRequest implementation
 */

namespace GoogleLogin\Auth;

use GoogleLogin\GoogleUser;
use MediaWiki\Auth\AuthManager;
use MediaWiki\Auth\ButtonAuthenticationRequest;

/**
 * Implements a GoogleAuthenticationRequest by extending a ButtonAuthenticationRequest
 * and describes the credentials used/needed by this AuthenticationRequest.
 */
class GoogleAuthenticationRequest extends ButtonAuthenticationRequest {
	public function __construct( \Message $label, \Message $help ) {

		parent::__construct(
			GooglePrimaryAuthenticationProvider::GOOGLELOGIN_BUTTONREQUEST_NAME,
			$label,
			$help,
			true
		);
	}

	public function getFieldInfo() {
		if ( $this->action === AuthManager::ACTION_REMOVE ) {
			return [];
		}
		return parent::getFieldInfo();
	}
}
