<?php

namespace GoogleLogin;

use ConfigFactory;
use Linker;
use SpecialPage;
use ChangeTags;

use GoogleLogin\Specials\SpecialGoogleLogin;

class GoogleLoginHooks {
	public static function onUserLogoutComplete() {
		global $wgRequest;

		if ( $wgRequest->getSessionData( 'access_token' ) !== null ) {
			$wgRequest->setSessionData( 'access_token', null );
		}
	}

	public static function onLoadExtensionSchemaUpdates( \DatabaseUpdater $updater = null ) {
		$config = ConfigFactory::getDefaultInstance()->makeConfig( 'main' );
		// Don't create tables on a shared database
		$sharedDB = $config->get( 'SharedDB' );
		if (
			!empty( $sharedDB ) &&
			$sharedDB !== $config->get( 'DBname' )
		) {
			return true;
		}

		// Sql directory inside the extension folder
		$sql = __DIR__ . '/sql';
		$schema = "$sql/user_google_user.sql";
		$updater->addExtensionUpdate( [ 'addTable', 'user_google_user', $schema, true ] );
		if ( !$updater->getDB()->indexExists( 'user_google_user', 'user_id' ) ) {
			$updater->modifyExtensionField( 'user_google_user',
				'user_id',
				"$sql/user_google_user_user_id_index.sql" );
		}
		return true;
	}

	/**
	 * Handles Updates to the UserMergeAccountFields of the UserMerge extension.
	 *
	 * @param array &$updateFields
	 */
	public static function onMergeAccountFromTo( &$oldUser, &$newUser ) {
		// check, if
		if (
			// the new user exists (e.g. is not Anonymous)
			!$newUser->isAnon() &&
			// the new user doesn't has a google connection already
			!$newUser->hasConnectedGoogleAccount() &&
			// the old user has a google connection
			$oldUser->hasConnectedGoogleAccount()
		) {
			// save the google id of the old account
			$googleIds = GoogleUser::getGoogleIdFromUser( $oldUser );
			foreach ( $googleIds as $i => $id ) {
				// delete the connection between the google and the old wiki account
				GoogleUser::terminateGoogleConnection( $oldUser, $id );
				// add the google id to the new account
				GoogleUser::connectWithGoogle( $newUser, $id );
			}
		}

		return true;
	}

	/**
	 * Handle, what data needs to be deleted from the GoogleLogin tables when a user is
	 * deleted through the UserMerge extension.
	 *
	 * @param array &$tablesToDelete
	 */
	public static function onUserMergeAccountDeleteTables( &$tablesToDelete ) {
		$tablesToDelete['user_google_user'] = 'user_id';

		return true;
	}

	/**
	 * AuthChangeFormFields hook handler. Give the "Login with Google" button a larger
	 * weight as the LocalPasswordAuthentication Log in button.
	 *
	 * @param array $requests
	 * @param array $fieldInfo
	 * @param array $formDescriptor
	 * @param $action
	 */
	public static function onAuthChangeFormFields( array $requests, array $fieldInfo,
		array &$formDescriptor, $action
	) {
		if ( isset( $formDescriptor['googlelogin'] ) ) {
			$formDescriptor['googlelogin'] = array_merge( $formDescriptor['googlelogin'],
				[
					'weight' => 101,
					'flags' => [],
					'class' => HTMLGoogleLoginButtonField::class
				]
			);
			unset( $formDescriptor['googlelogin']['type'] );
		}
	}
}
