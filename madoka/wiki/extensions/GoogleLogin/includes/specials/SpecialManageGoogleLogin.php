<?php
/**
 * SpecialManageGoogleLogin implementation
 */

namespace GoogleLogin\Specials;

use SpecialPage;
use Html;
use HTMLForm;
use User;
use ConfigFactory;
use Http;
use ErrorPageError;

use GoogleLogin\GoogleUser;

/**
 * Special page implementation that allows a permitted user to manage
 * connections between wiki accounts and Google accounts.
 */
class SpecialManageGoogleLogin extends SpecialPage {
	/** @var User $manageableUser User object of the user to manage */
	private $manageableUser = null;

	function __construct() {
		parent::__construct( 'ManageGoogleLogin', 'managegooglelogin' );
		$this->listed = true;
	}

	public function doesWrites() {
		return true;
	}

	/**
	 * Special page executer
	 * @param SubPage $par Subpage
	 */
	function execute( $par ) {
		$user = $this->getUser();
		$out = $this->getOutput();
		$request = $this->getRequest();
		if ( !$this->userCanExecute( $user ) ) {
			$this->displayRestrictionError();
			return;
		}
		$this->setHeaders();
		$out->addModules( 'mediawiki.userSuggest' );
		$formFields = [
			'username' => [
				'type' => 'user',
				'name' => 'username',
				'label-message' => 'googlelogin-username',
				'id' => 'mw-gl-username',
				'autofocus' => true,
				'exists' => true,
			],
			'submit' => [
				'type' => 'submit',
				'default' => $this->msg( 'googlelogin-manage-usersubmit' )->text(),
				'flags' => [ 'progressive', 'primary' ],
			],
		];
		$htmlForm = HTMLForm::factory( 'ooui', $formFields, $this->getContext(), 'googlelogin-manage' );
		$htmlForm->setWrapperLegendMsg( $this->msg( 'googlelogin-managelegend' ) );
		$htmlForm->suppressDefaultSubmit();
		$htmlForm->setSubmitCallback( [ $this, 'submitUserName' ] );
		$htmlForm->show();

		if ( $this->manageableUser ) {
			$this->manageUser( $this->manageableUser );
		}
	}

	/**
	 * Checks, if a user with the entered username exists.
	 *
	 * @param array $data Formdata
	 * @return boolean
	 */
	public function submitUserName( array $data ) {
		$this->submitForm( $data, false );
		return true;
	}

	/**
	 * Renders a form to manage this user and handles all actions.
	 *
	 * @param User $user
	 */
	private function manageUser( User $user ) {
		$out = $this->getOutput();

		$out->addModules(
			[
				'ext.GoogleLogin.specialManage.scripts',
				'ext.GoogleLogin.style',
			]
		);
		$out->addBackLinkSubtitle( $this->getPageTitle() );

		$out->addWikiMsg( 'googlelogin-manage-user', $user->getName() );
		$googleIds = GoogleUser::getGoogleIdFromUser( $user );
		if ( $googleIds ) {
			$out->addHTML(
				Html::openElement( 'div' ) .
				$this->msg( 'googlelogin-manage-linked' )->escaped() .
				Html::openElement( 'strong' )
			);

			foreach ( $googleIds as $count => $googleId ) {
				if ( $count !== 0 ) {
					$out->addHTML( ', ' );
				}
				$out->addHTML(
					Html::element( 'a',
						[
							'href' => 'javascript:void(0)',
							'data-googleid' => $googleId,
						],
						$googleId
					) );
			}
			$out->addHTML(
				Html::closeElement( 'strong' ) .
				Html::closeElement( 'div' )
			);
		} else {
			$out->addWikiMsg( 'googlelogin-manage-notlinked' );
		}
		$formFields = [];
		if ( $googleIds ) {
			$formFields['googleids'] =  [
				'type' => 'checkmatrix',
				'columns' => [
					$this->msg( 'googlelogin-manage-terminatebutton' )->escaped() => 'google'
				],
				'rows' => array_combine( $googleIds, $googleIds ),
				'label-raw' => 'Google ID',
				'id' => 'mw-gl-username',
			];
		}
		$formFields['addgoogleid'] = [
			'type' => 'text',
			'label-message' => 'googlelogin-manage-addlabel',
		];
		$htmlForm = HTMLForm::factory( 'ooui', $formFields, $this->getContext(), 'googlelogin-change' );
		$htmlForm->addHiddenField( 'username', $user->getName() );
		$htmlForm->setWrapperLegendMsg( $this->msg( 'googlelogin-manage-changelegend' ) );
		$htmlForm->setSubmitCallback( [ $this, 'submitForm' ] );
		$htmlForm->show();
	}

	/**
	 * Submithandler for new google id
	 *
	 * @param array $data Formdata
	 * @param boolean $checkSession If true, checks, if the form was submitted by the user itself
	 * @return boolean
	 */
	public function submitForm( array $data, $checkSession = true ) {
		$user = $this->getUser();
		$request = $this->getRequest();
		$out = $this->getOutput();
		$name = ( isset( $data['username'] ) ? $data['username'] : '' );
		if ( $checkSession && !$user->matchEditToken( $request->getVal( 'wpEditToken' ), $name ) ) {
			throw new ErrorPageError( 'sessionfailure-title', 'sessionfailure' );
		}
		if ( $name ) {
			$this->manageableUser = User::newFromName( $name );
		}
		if ( !$this->manageableUser ) {
			return false;
		}
		$requestAddGoogleId = null;
		$requestGoogleId = null;
		if ( isset( $data['googleids'] ) ) {
			$requestGoogleId = $data['googleids'];
		}
		if ( isset( $data['addgoogleid'] ) ) {
			$requestAddGoogleId = $data['addgoogleid'];
		}
		if ( isset( $requestGoogleId ) && GoogleUser::hasConnectedGoogleAccount( $user ) ) {
			// terminate the connection
			foreach ( $requestGoogleId as $count => $googleId ) {
				$id = str_replace( 'google-', '', $googleId );
				if ( GoogleUser::terminateGoogleConnection( $user, $id ) ) {
					$out->addWikiMsg( 'googlelogin-manage-terminatesuccess' );
				} else {
					$out->addWikiMsg( 'googlelogin-manage-changederror' );
				}
			}
		}

		if ( $requestAddGoogleId ) {
			// try to create a new GoogleUser object with the given id to check, if there's
			// already an user with this google id.
			$newGoogleUser = GoogleUser::isGoogleIdFree( $requestAddGoogleId );
			if ( !is_numeric( $requestAddGoogleId ) ) {
				$out->wrapWikiMsg( '<div class="error">$1</div>', 'googlelogin-manage-invalidid' );
			} elseif ( !$newGoogleUser ) {
				// if the ID is already given to another user, it can't be associtated with this user
				$out->wrapWikiMsg( '<div class="error">$1</div>', 'googlelogin-manage-givenid' );
			} else {
				// check, if the google id has a google plus profile
				$glConfig = ConfigFactory::getDefaultInstance()->makeConfig( 'googlelogin' );
				$plusCheck = Http::get(
					'https://www.googleapis.com/plus/v1/people/' .
					$requestAddGoogleId .
					'?key=' .
					$glConfig->get( 'GLAPIKey' )
				);
				if ( !$plusCheck ) {
					// it seems, that the google id doesn't have a plus profile
					// (or another error occur). Add a notice message for it.
					$out->addWikiMsg( 'googlelogin-manage-noplus' );
				}

				if ( GoogleUser::connectWithGoogle( $user, $requestAddGoogleId ) ) {
					$out->addWikiMsg( 'googlelogin-manage-changedsuccess' );
				} else {
					$out->addWikiMsg( 'googlelogin-manage-changederror' );
				}
			}
		}
		return false;
	}

	protected function getGroupName() {
		return 'users';
	}
}
