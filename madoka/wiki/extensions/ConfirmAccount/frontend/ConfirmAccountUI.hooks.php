<?php
/**
 * Class containing hooked functions for a ConfirmAccount environment
 */
class ConfirmAccountUIHooks {
	/**
	 * @param $template
	 * @return bool
	 */
	public static function addRequestLoginText( &$template ) {
		$context = RequestContext::getMain();
		# Add a link to RequestAccount from UserLogin
		if ( !$context->getUser()->isAllowed( 'createaccount' ) ) {
			$template->set( 'header', $context->msg( 'requestaccount-loginnotice' )->parseAsBlock() );

			$context->getOutput()->addModules( 'ext.confirmAccount' ); // CSS
		}
		return true;
	}

	/**
	 * @param $personal_urls
	 * @param $title
	 * @param $skin
	 * @return bool
	 */
	public static function setRequestLoginLinks(
		array &$personal_urls, Title $title, SkinTemplate $skin
	) {
		# Add a link to Special:RequestAccount if a link exists for login
		if ( isset( $personal_urls['login'] ) ) {
			$personal_urls['createaccount'] = [
				'text' => wfMessage( 'requestaccount-login' )->text(),
				'href' => SpecialPage::getTitleFor( 'RequestAccount' )->getLocalUrl()
			];
		}
		return true;
	}

	/**
	 * Add "x email-confirmed open account requests" notice
	 * @param OutputPage $out
	 * @param Skin $skin
	 * @return bool
	 */
	public static function confirmAccountsNotice( OutputPage &$out, Skin &$skin ) {
		global $wgConfirmAccountNotice;

		$context = $out->getContext();
		if ( !$wgConfirmAccountNotice || !$context->getUser()->isAllowed( 'confirmaccount' ) ) {
			return true;
		}
		# Only show on some special pages
		$title = $context->getTitle();
		if ( !$title->isSpecial( 'Recentchanges' ) && !$title->isSpecial( 'Watchlist' ) ) {
			return true;
		}
		$count = ConfirmAccount::getOpenEmailConfirmedCount( '*' );
		if ( $count > 0 ) {
			$out->prependHtml(
				'<div id="mw-confirmaccount-msg" class="plainlinks mw-confirmaccount-bar">' .
				$context->msg( 'confirmaccount-newrequests' )->numParams( $count )->parse() .
				'</div>'
			);

			$out->addModules( 'ext.confirmAccount' ); // CSS
		}
		return true;
	}

	/**
	 * For AdminLinks extension
	 * @param $admin_links_tree
	 * @return bool
	 */
	public static function confirmAccountAdminLinks( &$admin_links_tree ) {
		$users_section = $admin_links_tree->getSection( wfMessage( 'adminlinks_users' )->escaped() );
		$extensions_row = $users_section->getRow( 'extensions' );

		if ( is_null( $extensions_row ) ) {
			$extensions_row = new ALRow( 'extensions' );
			$users_section->addRow( $extensions_row );
		}

		$extensions_row->addItem( ALItem::newFromSpecialPage( 'ConfirmAccounts' ) );
		$extensions_row->addItem( ALItem::newFromSpecialPage( 'UserCredentials' ) );

		return true;
	}

	/**
	 * @param array $requests
	 * @param array $fieldInfo
	 * @param array $formDescriptor
	 * @param string $action
	 * @return bool
	 * @throws ErrorPageError
	 */
	public static function onAuthChangeFormFields(
		$requests, $fieldInfo, array &$formDescriptor, $action
	) {
		if ( $action !== \MediaWiki\Auth\AuthManager::ACTION_CREATE ) {
			return true;
		}

		$request = RequestContext::getMain()->getRequest();
		$accReqId = $request->getInt( 'AccountRequestId' );
		$isAllowed = RequestContext::getMain()->getUser()->isAllowed( 'confirmaccount' );
		if ( $accReqId && $isAllowed ) {
			$accReq = UserAccountRequest::newFromId( $accReqId );
			if ( !$accReq ) {
				throw new ErrorPageError( 'createacct-error', 'confirmaccount-badid' );
			}
		} else {
			return true;
		}

		$formDescriptor['username']['default'] = $accReq->getName();

		$formDescriptor['mailpassword']['default'] = 1;
		$formDescriptor['mailpassword']['checked'] = true;
		$formDescriptor['mailpassword']['readonly'] = true;
		$formDescriptor['mailpassword']['validation-callback'] = function ( $v ) use ( $accReq ) {
			return ( $v === true )
				? true
				: wfMessage( 'confirmaccount-mismatched' );
		};

		unset( $formDescriptor['password'] );
		unset( $formDescriptor['retype'] );

		$formDescriptor['email']['default'] = $accReq->getEmail();
		$formDescriptor['email']['readonly'] = true;
		$formDescriptor['email']['validation-callback'] = function ( $v ) use ( $accReq ) {
			return ( $v === $accReq->getEmail() )
				? true
				: wfMessage( 'confirmaccount-mismatched' );
		};

		$formDescriptor['realname']['default'] = $accReq->getRealName();
		$formDescriptor['realname']['readonly'] = true;
		$formDescriptor['realname']['validation-callback'] = function ( $v ) use ( $accReq ) {
			return ( $v === $accReq->getRealName() )
				? true
				: wfMessage( 'confirmaccount-mismatched' );
		};

		$formDescriptor['accountrequestid'] = [
			'name' => 'AccountRequestId',
			'type' => 'hidden',
			'default' => $accReqId
		];

		return true;
	}
}
