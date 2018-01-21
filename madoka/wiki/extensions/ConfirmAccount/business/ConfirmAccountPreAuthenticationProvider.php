<?php
/**
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program; if not, write to the Free Software Foundation, Inc.,
 * 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301, USA.
 * http://www.gnu.org/copyleft/gpl.html
 *
 * @file
 * @ingroup Auth
 */

use \MediaWiki\Auth\AbstractPreAuthenticationProvider;
use \MediaWiki\Auth\AuthenticationRequest;
use \MediaWiki\Auth\AuthenticationResponse;
use \MediaWiki\MediaWikiServices;

class ConfirmAccountPreAuthenticationProvider extends AbstractPreAuthenticationProvider {
	const SESSION_INFO_KEY = 'ConfirmAccountRequestInfo';

	/**
	 * @param \User $user
	 * @param \User $creator
	 * @param array $reqs
	 * @return bool
	 * @throws MWException
	 * @TODO: avoid using global WebRequest and use dedicate auth request class
	 */
	public function testForAccountCreation( $user, $creator, array $reqs ) {
		$request = RequestContext::getMain()->getRequest();
		$accReqId = $request->getInt( 'AccountRequestId' );
		# For normal account creations, just check if the name is free
		if ( !$accReqId ) {
			return UserAccountRequest::acquireUsername( $user->getName() )
				? StatusValue::newGood() // no pending requests with this name
				: StatusValue::newFatal( 'requestaccount-inuse' );
		}

		# User is doing a confirmation creation...

		# Allow creations for account requests as long as the parameters match up.
		# Always keep names reserved on API requests as there is no API support for now.
		if ( !$creator->isAllowed( 'confirmaccount' ) || defined( 'MW_API' ) ) {
			return StatusValue::newFatal( 'badaccess-group0' );
		}

		$accountReq = UserAccountRequest::newFromId( $accReqId );
		if ( !$accountReq ) {
			return StatusValue::newFatal( 'confirmaccount-badid' );
		}

		/** @var \MediaWiki\Auth\UserDataAuthenticationRequest $usrDataAuthReq */
		$usrDataAuthReq = AuthenticationRequest::getRequestByClass(
			$reqs, \MediaWiki\Auth\UserDataAuthenticationRequest::class );
		/** @var \MediaWiki\Auth\TemporaryPasswordAuthenticationRequest $tmpPassAuthReq */
		$tmpPassAuthReq = AuthenticationRequest::getRequestByClass(
			$reqs, \MediaWiki\Auth\TemporaryPasswordAuthenticationRequest::class );

		# Make sure certain field were left unchanged from the account request
		if (
			!$tmpPassAuthReq ||
			$usrDataAuthReq->email !== $accountReq->getEmail() ||
			$usrDataAuthReq->realname !== $accountReq->getRealName() ||
			!$tmpPassAuthReq->mailpassword
		) {
			return StatusValue::newFatal( 'confirmaccount-mismatched-general' );
		}

		$this->manager->setAuthenticationSessionData(
			self::SESSION_INFO_KEY,
			[
				'accountRequestId' => $accountReq->getId(),
				'confirmationParams' => [
					'userName' => $request->getVal( 'wpName', $user->getName() ),
					'action' => 'complete',
					'reason' => $request->getVal( 'wpReason', '' ),
					// @TODO: make overridable in GUI
					'bio' => $request->getText( 'wpNewBio', $accountReq->getBio() ),
					'type' => $request->getInt( 'wpType', $accountReq->getType() ),
					// @TODO: make overridable
					'areas' => $accountReq->getAreas(),
					'allowComplete' => true // action not enabled via GUI
				]
			]
		);

		return StatusValue::newGood();
	}

	public function postAccountCreation( $user, $creator, AuthenticationResponse $response ) {
		if ( $response->status === AuthenticationResponse::FAIL ) {
			return; // nothing happened
		}

		$data = $this->manager->getAuthenticationSessionData( self::SESSION_INFO_KEY );
		if ( !$data ) {
			return; // wasn't for a pending account request
		}

		$submission = new AccountConfirmSubmission(
			$creator,
			UserAccountRequest::newFromId( $data['accountRequestId'], 'dbmaster' ),
			$data['confirmationParams']
		);

		# Update the queue to reflect approval of this user
		list( $status, $msg ) = $submission->submit( RequestContext::getMain() );
		if ( $status !== true ) {
			// ErrorPageError does not trigger rollback
			$lbFactory = MediaWikiServices::getInstance()->getDBLoadBalancerFactory();
			$lbFactory->rollbackMasterChanges( __METHOD__ );
			throw new ErrorPageError( 'createacct-error', new RawMessage( $msg ) );
		}

		return;
	}
}
