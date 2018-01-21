<?php

class UserCredentialsPage extends SpecialPage {
	protected $target, $file;

	function __construct() {
		parent::__construct( 'UserCredentials', 'lookupcredentials' );
	}

	public function userCanExecute( User $user ) {
		global $wgConfirmAccountSaveInfo;
		return $wgConfirmAccountSaveInfo && parent::userCanExecute( $user );
	}

	function execute( $par ) {
		$out = $this->getOutput();
		$request = $this->getRequest();
		$reqUser = $this->getUser();

		if ( !$this->userCanExecute( $this->getUser() ) ) {
			throw new PermissionsError( 'lookupcredentials' );
		}

		$this->setHeaders();

		# A target user
		$this->target = $request->getText( 'target' );
		# Attachments
		$this->file = $request->getVal( 'file' );

		if ( $this->file ) {
			$this->showFile( $this->file );
		} elseif ( $this->target ) {
			$this->showForm();
			$this->showCredentials();
		} else {
			$this->showForm();
		}
		$out->addModules( 'ext.confirmAccount' ); // CSS
	}

	function showForm() {
		global $wgScript;
		$out = $this->getOutput();

		$username = str_replace( '_', ' ', $this->target );
		$form = Xml::openElement(
			'form', [ 'name' => 'stablization', 'action' => $wgScript, 'method' => 'get' ]
		);
		$form .= "<fieldset><legend>" . $this->msg( 'usercredentials-leg' )->escaped() . "</legend>";
		$form .= "<table><tr>";
		$form .= "<td>" . Html::Hidden( 'title', $this->getPageTitle()->getPrefixedText() ) . "</td>";
		$form .= "<td>" . $this->msg( "usercredentials-user" )->escaped() . "</td>";
		$form .= "<td>" . Xml::input( 'target', 35, $username, [ 'id' => 'wpUsername' ] ) . "</td>";
		$form .= "<td>" . Xml::submitButton( $this->msg( 'go' )->text() ) . "</td>";
		$form .= "</tr></table>";
		$form .= "</fieldset></form>\n";

		$out->addHTML( $form );
	}

	function showCredentials() {
		$reqUser = $this->getUser();
		$out = $this->getOutput();

		$titleObj = SpecialPage::getTitleFor( 'UserCredentials' );

		$row = $this->getAccountData();
		if ( !$row ) {
			$out->addHTML( $this->msg( 'usercredentials-badid' )->escaped() );
			return;
		}

		$out->addWikiMsg( 'usercredentials-text' );

		$user = User::newFromName( $this->target );

		$list = [];
		foreach ( $user->getGroups() as $group ) {
			$list[] = User::makeGroupLinkHTML(
				$group,
				User::getGroupMember( $group, $user->getName() )
			);
		}

		$grouplist = '';
		if ( count( $list ) > 0 ) {
			$grouplist = '<tr><td>' . $this->msg(
				'usercredentials-member'
			)->escaped() . '</td><td>' . implode( ', ', $list ) . '</td></tr>';
		}

		$form  = "<fieldset>";
		$form .= '<legend>' . $this->msg( 'usercredentials-leg-user' )->escaped() . '</legend>';
		$form .= '<table style="padding:4px;">';
		$form .= "<tr><td>" . $this->msg( 'username' )->escaped() . "</td>";
		$linkRenderer = $this->getLinkRenderer();
		$form .= "<td>" . $linkRenderer->makeLink(
			$user->getUserPage(),
			$user->getUserPage()->getText()
		) . "</td></tr>\n";

		$econf = $row->acd_email_authenticated ? ' <strong>' . $this->msg(
			'confirmaccount-econf'
		)->escaped() . '</strong>' : '';
		$form .= "<tr><td>" . $this->msg( 'usercredentials-email' )->escaped() . "</td>";
		$form .= "<td>" . htmlspecialchars( $row->acd_email ) . $econf . "</td></tr>\n";

		$form .= $grouplist;

		$form .= '</table></fieldset>';

		$areaSet = UserAccountRequest::expandAreas( $row->acd_areas );

		$userAreas = ConfirmAccount::getUserAreaConfig();
		if ( count( $userAreas ) > 0 ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'confirmaccount-leg-areas' )->escaped() . '</legend>';

			$form .= "<div style='height:150px; overflow:scroll; background-color:#f9f9f9;'>";
			$form .= "<table style='border-spacing: 5px; padding: 0px; background-color: #f9f9f9;'>
			<tr style='vertical-align: top;'>";
			$count = 0;

			$att = [ 'disabled' => 'disabled' ];
			foreach ( $userAreas as $name => $conf ) {
				$count++;
				if ( $count > 5 ) {
					$form .= "</tr><tr style='vertical-align: top;'>";
					$count = 1;
				}
				$formName = "wpArea-" . htmlspecialchars( str_replace( ' ', '_', $name ) );
				if ( $conf['project'] != '' ) {
					$linkRenderer = $this->getLinkRenderer();
					$pg = $linkRenderer->makeKnownLink(
						Title::newFromText( $name ),
						$this->msg( 'requestaccount-info' )->text()
					);
				} else {
					$pg = '';
				}
				$form .= "<td>" .
					Xml::checkLabel( $name, $formName, $formName, in_array( $formName, $areaSet ), $att ) .
					" {$pg}</td>\n";
			}
			$form .= "</tr></table></div>";
			$form .= '</fieldset>';
		}

		$form .= '<fieldset>';
		$form .= '<legend>' . $this->msg( 'usercredentials-leg-person' )->escaped() . '</legend>';
		$form .= '<table style="padding:4px;">';
		$form .= "<tr><td>" . $this->msg( 'usercredentials-real' )->escaped() . "</td>";
		$form .= "<td>" . htmlspecialchars( $row->acd_real_name ) . "</td></tr>\n";
		$form .= '</table>';
		$form .= "<p>" . $this->msg( 'usercredentials-bio' )->escaped() . "</p>";
		$form .= "<p><textarea tabindex='1' readonly='readonly' name='wpBio'
		id='wpNewBio' rows='10' cols='80' style='width: 100%'>" .
			htmlspecialchars( $row->acd_bio ) .
			"</textarea></p>\n";
		$form .= '</fieldset>';

		$form .= '<fieldset>';
		$form .= '<legend>' . $this->msg( 'usercredentials-leg-other' )->escaped() . '</legend>';
		if ( $this->hasItem( 'CV' ) || $this->hasItem( 'Notes' ) || $this->hasItem( 'Links' ) ) {
			$form .= '<p>' . $this->msg( 'usercredentials-attach' )->escaped() . ' ';
			if ( $row->acd_filename ) {
				$linkRenderer = $this->getLinkRenderer();
				$form .= $linkRenderer->makeKnownLink(
					$titleObj,
					$row->acd_filename,
					[],
					[ 'file' => $row->acd_storage_key ]
				);
			} else {
				$form .= $this->msg( 'confirmaccount-none-p' )->escaped();
			}
			$form .= "</p><p>" . $this->msg( 'usercredentials-notes' )->escaped() . "</p>\n";
			$form .= "<p><textarea tabindex='1' readonly='readonly' name='wpNotes'
			id='wpNotes' rows='3' cols='80' style='width: 100%'>" .
				htmlspecialchars( $row->acd_notes ) .
				"</textarea></p>\n";
			$form .= "<p>" . $this->msg( 'usercredentials-urls' )->escaped() . "</p>\n";
			$form .= ConfirmAccountsPage::parseLinks( $row->acd_urls );
		}
		$form .= '</fieldset>';

		if ( $reqUser->isAllowed( 'requestips' ) ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'usercredentials-leg-ip' )->escaped() . '</legend>';
			$form .= "<p>" . $this->msg( 'usercredentials-ip' )->escaped() .
				" " . htmlspecialchars( $row->acd_ip ) . "</p>\n";
			if ( $row->acd_xff ) {
				$form .= "<p>" . $this->msg( 'usercredentials-xff' )->escaped() .
					" " . htmlspecialchars( $row->acd_xff ) . "</p>\n";
			}
			if ( $row->acd_agent ) {
				$form .= "<p>" . $this->msg( 'usercredentials-agent' )->escaped() .
					" " . htmlspecialchars( $row->acd_agent ) . "</p>\n";
			}
			$form .= '</fieldset>';
		}

		$out->addHTML( $form );
	}

	protected function hasItem( $name ) {
		global $wgConfirmAccountRequestFormItems;

		return $wgConfirmAccountRequestFormItems[$name]['enabled'];
	}

	/**
	 * Show a private file requested by the visitor.
	 * @param $key string
	 * @return void
	 */
	function showFile( $key ) {
		global $wgConfirmAccountFSRepos;
		$out = $this->getOutput();
		$request = $this->getRequest();

		$out->disable();

		# We mustn't allow the output to be Squid cached, otherwise
		# if an admin previews a private image, and it's cached, then
		# a user without appropriate permissions can toddle off and
		# nab the image, and Squid will serve it
		$request->response()->header( 'Expires: ' . gmdate( 'D, d M Y H:i:s', 0 ) . ' GMT' );
		$request->response()->header( 'Cache-Control: no-cache, no-store, max-age=0, must-revalidate' );
		$request->response()->header( 'Pragma: no-cache' );

		$repo = new FSRepo( $wgConfirmAccountFSRepos['accountcreds'] );
		$path = $repo->getZonePath( 'public' ) . '/' .
			UserAccountRequest::relPathFromKey( $key );

		$repo->streamFile( $path );
	}

	function getAccountData() {
		$uid = User::idFromName( $this->target );
		if ( !$uid ) {
			return false;
		}
		# For now, just get the first revision...
		$dbr = wfGetDB( DB_SLAVE );
		$row = $dbr->selectRow( 'account_credentials', '*',
			[ 'acd_user_id' => $uid ],
			__METHOD__,
			[ 'ORDER BY' => 'acd_user_id,acd_id ASC' ] );
		return $row;
	}

	protected function getGroupName() {
		return 'users';
	}
}
