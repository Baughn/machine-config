<?php
/** \file
 * \brief Contains code for the UserMerge Class (extends SpecialPage).
 */

/**
 * Special page class for the User Merge and Delete extension
 * allows sysops to merge references from one user to another user.
 * It also supports deleting users following merge.
 *
 * @ingroup Extensions
 * @author Tim Laqua <t.laqua@gmail.com>
 * @author Thomas Gries <mail@tgries.de>
 * @author Matthew April <Matthew.April@tbs-sct.gc.ca>
 *
 */

class SpecialUserMerge extends FormSpecialPage {
	public function __construct() {
		parent::__construct( 'UserMerge', 'usermerge' );
	}

	/**
	 * @return array
	 */
	protected function getFormFields() {
		$us = $this;
		return [
			'olduser' => [
				'type' => 'text',
				'label-message' => 'usermerge-olduser',
				'required' => true,
				'validation-callback' => function( $val ) use ( $us ) {
					// only pass strings to User::newFromName
					if ( !is_string( $val ) ) {
						return true;
					}

					$key = $us->validateOldUser( $val );
					if ( is_string( $key ) || is_array( $key ) ) {
						return $us->msg( $key )->escaped();
					}
					return true;
				},
			],
			'newuser' => [
				'type' => 'text',
				'required' => true,
				'label-message' => 'usermerge-newuser',
				'validation-callback' => function( $val ) use ( $us ) {
					// only pass strings to User::newFromName
					if ( !is_string( $val ) ) {
						return true;
					}

					$key = $us->validateNewUser( $val );
					if ( is_string( $key ) ) {
						return $us->msg( $key )->escaped();
					}
					return true;
				},
			],
			'delete' => [
				'type' => 'check',
				'label-message' => 'usermerge-deleteolduser',
			],
		];
	}

	/**
	 * @param $val user's input for username
	 * @return bool|string true if valid, a string of the error's message key if validation failed
	 */
	public function validateOldUser( $val ) {
		global $wgUserMergeProtectedGroups;
		$oldUser = User::newFromName( $val );
		if ( !$oldUser || $oldUser->getId() === 0 ) {
			return 'usermerge-badolduser';
		}
		if ( $this->getUser()->getId() === $oldUser->getId() ) {
			return [ 'usermerge-noselfdelete', $this->getUser()->getName() ];
		}
		if ( count( array_intersect( $oldUser->getGroups(), $wgUserMergeProtectedGroups ) ) ) {
			return [ 'usermerge-protectedgroup', $oldUser->getName() ];
		}

		return true;
	}

	/**
	 * @param $val user's input for username
	 * @return bool|string true if valid, a string of the error's message key if validation failed
	 */
	public function validateNewUser( $val ) {
		global $wgUserMergeEnableDelete;
		if ( $wgUserMergeEnableDelete && $val === 'Anonymous' ) {
			return true; // Special case
		}
		$newUser = User::newFromName( $val );
		if ( !$newUser || $newUser->getId() === 0 ) {
			return 'usermerge-badnewuser';
		}

		return true;
	}

	/**
	 * @param HTMLForm $form
	 */
	protected function alterForm( HTMLForm $form ) {
		$form->setSubmitTextMsg( 'usermerge-submit' );
		$form->setWrapperLegendMsg( 'usermerge-fieldset' );
	}

	/**
	 * @param array $data
	 * @return Status
	 */
	public function onSubmit( array $data ) {
		global $wgUserMergeEnableDelete;
		// Most of the data has been validated using callbacks
		// still need to check if the users are different
		$newUser = User::newFromName( $data['newuser'] );
		// Handle "Anonymous" as a special case for user deletion
		if ( $wgUserMergeEnableDelete && $data['newuser'] === 'Anonymous' ) {
			$newUser->mId = 0;
		}

		$oldUser = User::newFromName( $data['olduser'] );
		if ( $newUser->getName() === $oldUser->getName() ) {
			return Status::newFatal( 'usermerge-same-old-and-new-user' );
		}

		// Validation passed, let's merge the user now.
		$um = new MergeUser( $oldUser, $newUser, new UserMergeLogger() );
		$um->merge( $this->getUser(), __METHOD__ );

		$out = $this->getOutput();

		$out->addWikiMsg(
			'usermerge-success',
			$oldUser->getName(), $oldUser->getId(),
			$newUser->getName(), $newUser->getId()
		);

		if ( $data['delete'] ) {
			$failed = $um->delete( $this->getUser(), [ $this, 'msg' ] );
			$out->addWikiMsg(
				'usermerge-userdeleted', $oldUser->getName(), $oldUser->getId()
			);

			if ( $failed ) {
				// Output an error message for failed moves
				$out->addHTML( Html::openElement( 'ul' ) );
				foreach ( $failed as $oldTitleText => $newTitle ) {
					$oldTitle = Title::newFromText( $oldTitleText );
					$out->addHTML(
						Html::rawElement( 'li', [],
							$this->msg( 'usermerge-page-unmoved' )->rawParams(
								Linker::link( $oldTitle ),
								Linker::link( $newTitle )
							)->escaped()
						)
					);
				}
				$out->addHTML( Html::closeElement( 'ul' ) );
			}
		}

		return Status::newGood();
	}

	protected function getGroupName() {
		return 'users';
	}
}
