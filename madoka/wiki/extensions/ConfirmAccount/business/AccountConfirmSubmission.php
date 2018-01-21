<?php

use MediaWiki\Auth\AuthManager;

class AccountConfirmSubmission {
	/* User making the confirmation */
	protected $admin;
	/** @var UserAccountRequest */
	protected $accReq;
	/* Admin-overridable name and fields filled from request form */
	protected $userName;
	protected $bio;
	protected $type;
	/** @var array */
	protected $areas;

	protected $action;
	protected $reason;

	/** @var bool Enable dummy "complete" action */
	protected $allowComplete;

	public function __construct( User $admin, UserAccountRequest $accReq, array $params ) {
		$this->admin = $admin;
		$this->accountReq = $accReq;
		$this->userName = trim( $params['userName'] );
		$this->bio = trim( $params['bio'] );
		$this->type = $params['type'];
		$this->areas = $params['areas'];
		$this->action = $params['action'];
		$this->reason = $params['reason'];
		$this->allowComplete = !empty( $params['allowComplete'] );
	}

	/**
	 * Attempt to validate and submit this data to the DB
	 * @param $context IContextSource
	 * @return array( true or error key string, html error msg or null, redirect URL )
	 */
	public function submit( IContextSource $context ) {
		# Make sure that basic permissions are checked
		if ( !$this->admin->getID() || !$this->admin->isAllowed( 'confirmaccount' ) ) {
			return [
				'accountconf_permission_denied',
				$context->msg( 'badaccess-group0' )->escaped(),
				null
			];
		} elseif ( wfReadOnly() ) {
			return [
				'accountconf_readonly',
				$context->msg( 'badaccess-group0' )->escaped(),
				null
			];
		}
		if ( $this->action === 'spam' ) {
			return $this->spamRequest( $context );
		} elseif ( $this->action === 'reject' ) {
			return $this->rejectRequest( $context );
		} elseif ( $this->action === 'hold' ) {
			return $this->holdRequest( $context );
		} elseif ( $this->action === 'accept' ) {
			return $this->acceptRequest( $context );
		} elseif ( $this->action === 'complete' && $this->allowComplete ) {
			return $this->completeRequest( $context );
		} else {
			return [
				'accountconf_bad_action',
				$context->msg( 'confirmaccount-badaction' )->escaped(),
				null
			];
		}
	}

	protected function spamRequest( IContextSource $context ) {
		$dbw = wfGetDB( DB_MASTER );
		$dbw->startAtomic( __METHOD__ );

		$ok = $this->accountReq->markRejected( $this->admin, wfTimestampNow(), '' );
		if ( $ok ) {
			# Clear cache for notice of how many account requests there are
			ConfirmAccount::clearAccountRequestCountCache();
		}

		$dbw->endAtomic( __METHOD__ );
		return [ true, null, null ];
	}

	protected function rejectRequest( IContextSource $context ) {
		$dbw = wfGetDB( DB_MASTER );
		$dbw->startAtomic( __METHOD__ );

		$ok = $this->accountReq->markRejected( $this->admin, wfTimestampNow(), $this->reason );
		if ( $ok ) {
			# Make proxy user to email a rejection message :(
			$u = User::newFromName( $this->accountReq->getName(), false );
			$u->setEmail( $this->accountReq->getEmail() );
			# Send out a rejection email...
			if ( $this->reason != '' ) {
				$emailBody = $context->msg( 'confirmaccount-email-body4',
					$u->getName(), $this->reason )->inContentLanguage()->text();
			} else {
				$emailBody = $context->msg( 'confirmaccount-email-body3',
					$u->getName() )->inContentLanguage()->text();
			}
			$result = $u->sendMail(
				$context->msg( 'confirmaccount-email-subj' )->inContentLanguage()->text(),
				$emailBody
			);
			if ( !$result->isOk() ) {
				wfGetLBFactory()->rollbackMasterChanges( __METHOD__ );
				return [
					'accountconf_mailerror',
					$context->msg( 'mailerror' )->rawParams(
						$context->getOutput()->parse( $result->getWikiText() )
					)->text(),
					null
				];
			}
			# Clear cache for notice of how many account requests there are
			ConfirmAccount::clearAccountRequestCountCache();
		}

		$dbw->endAtomic( __METHOD__ );
		return [ true, null, null ];
	}

	protected function holdRequest( IContextSource $context ) {
		# Make proxy user to email a message
		$u = User::newFromName( $this->accountReq->getName(), false );
		$u->setEmail( $this->accountReq->getEmail() );

		# Pointless without a summary...
		if ( $this->reason == '' ) {
			return [
				'accountconf_needreason',
				$context->msg( 'confirmaccount-needreason' )->escaped(),
				null
			];
		}

		$dbw = wfGetDB( DB_MASTER );
		$dbw->startAtomic( __METHOD__ );

		# If not already held or deleted, mark as held
		$ok = $this->accountReq->markHeld( $this->admin, wfTimestampNow(), $this->reason );
		if ( !$ok ) { // already held or deleted?
			wfGetLBFactory()->rollbackMasterChanges( __METHOD__ );
			return [
				'accountconf_canthold',
				$context->msg( 'confirmaccount-canthold' )->escaped(),
				null
			];
		}

		# Send out a request hold email...
		$result = $u->sendMail(
			$context->msg( 'confirmaccount-email-subj' )->inContentLanguage()->text(),
			$context->msg(
				'confirmaccount-email-body5', $u->getName(), $this->reason
			)->inContentLanguage()->text()
		);
		if ( !$result->isOk() ) {
			wfGetLBFactory()->rollbackMasterChanges( __METHOD__ );
			return [
				'accountconf_mailerror',
				$context->msg( 'mailerror' )->rawParams(
					$context->getOutput()->parse( $result->getWikiText() )
				)->text(),
				null
			];
		}

		# Clear cache for notice of how many account requests there are
		ConfirmAccount::clearAccountRequestCountCache();

		$dbw->endAtomic( __METHOD__ );
		return [ true, null, null ];
	}

	protected function acceptRequest( IContextSource $context ) {
		global $wgAccountRequestTypes;

		$id = $this->accountReq->getId();
		$type = $wgAccountRequestTypes[$this->accountReq->getType()][0];
		$redirTitle = SpecialPageFactory::getTitleForAlias( 'CreateAccount' );
		$returnTitle = SpecialPageFactory::getTitleForAlias( "ConfirmAccounts/{$type}" );
		$params = [
			'AccountRequestId' => $id,
			'wpName' => $this->userName,
			'returnto' => $returnTitle->getPrefixedDBkey(),
			'reason' => $this->reason
		];

		return [ true, null, $redirTitle->getFullURL( $params ) ];
	}

	protected function completeRequest( IContextSource $context ) {
		global $wgConfirmAccountSaveInfo;
		global $wgConfirmAccountRequestFormItems, $wgConfirmAccountFSRepos;

		$formConfig = $wgConfirmAccountRequestFormItems; // convience
		$accReq = $this->accountReq; // convenience

		# Now create user and check if the name is valid
		$user = User::newFromName( $this->userName, false );

		$dbw = wfGetDB( DB_MASTER );
		$dbw->startAtomic( __METHOD__ );

		# Grant any necessary rights (exclude blank or dummy groups)
		$group = self::getGroupFromType( $this->type );
		if ( $group != '' && $group != 'user' && $group != '*' ) {
			$user->addGroup( $group );
		}

		$acd_id = null; // used for rollback cleanup
		# Save account request data to credentials system
		if ( $wgConfirmAccountSaveInfo ) {
			$key = $accReq->getFileStorageKey();
			# Copy any attached files to new storage group
			if ( $formConfig['CV']['enabled'] && $key ) {
				$repoOld = new FSRepo( $wgConfirmAccountFSRepos['accountreqs'] );
				$repoNew = new FSRepo( $wgConfirmAccountFSRepos['accountcreds'] );

				$pathRel = UserAccountRequest::relPathFromKey( $key );
				$oldPath = $repoOld->getZonePath( 'public' ) . '/' . $pathRel;

				$triplet = [ $oldPath, 'public', $pathRel ];
				$status = $repoNew->storeBatch( [ $triplet ] ); // copy!
				if ( !$status->isOK() ) {
					wfGetLBFactory()->rollbackMasterChanges( __METHOD__ );
					return [
						'accountconf_copyfailed',
						$context->getOutput()->parse( $status->getWikiText() ),
						null
					];
				}
			}
			$acd_id = $dbw->nextSequenceValue( 'account_credentials_acd_id_seq' );
			# Move request data into a separate table
			$dbw->insert( 'account_credentials',
				[
					'acd_user_id' => $user->getID(),
					'acd_real_name' => $accReq->getRealName(),
					'acd_email' => $accReq->getEmail(),
					'acd_email_authenticated' =>
						$dbw->timestampOrNull( $accReq->getEmailAuthTimestamp() ),
					'acd_bio' => $accReq->getBio(),
					'acd_notes' => $accReq->getNotes(),
					'acd_urls' => $accReq->getUrls(),
					'acd_ip' => $accReq->getIP(),
					'acd_xff' => $accReq->getXFF(),
					'acd_agent' => $accReq->getAgent(),
					'acd_filename' => $accReq->getFileName(),
					'acd_storage_key' => $accReq->getFileStorageKey(),
					'acd_areas' => $accReq->getAreas( 'flat' ),
					'acd_registration' => $dbw->timestamp( $accReq->getRegistration() ),
					'acd_accepted' => $dbw->timestamp(),
					'acd_user' => $this->admin->getID(),
					'acd_comment' => $this->reason,
					'acd_id' => $acd_id
				],
				__METHOD__
			);
		}

		# OK, now remove the request from the queue
		$accReq->remove();

		# Commit this if we make past the CentralAuth system
		# and the groups are added. Next step is sending out an
		# email, which we cannot take back...
		$dbw->endAtomic( __METHOD__ );

		$that = $this;
		DeferredUpdates::addCallableUpdate(
			function () use ( $that, $user, $context, $group, $accReq ) {
				$that->doPostCommitNewUserUpdates( $user, $context, $group, $accReq );
			}
		);

		return [ true, null, null ];
	}

	public function doPostCommitNewUserUpdates(
		User $user, IContextSource $context, $group, UserAccountRequest $accReq
	) {
		global $wgConfirmAccountRequestFormItems, $wgConfirmAccountFSRepos;

		# Clear cache for notice of how many account requests there are
		ConfirmAccount::clearAccountRequestCountCache();

		# Delete any attached file and don't stop the whole process if this fails
		if ( $wgConfirmAccountRequestFormItems['CV']['enabled'] ) {
			$key = $accReq->getFileStorageKey();
			if ( $key ) {
				$repoOld = new FSRepo( $wgConfirmAccountFSRepos['accountreqs'] );
				$pathRel = UserAccountRequest::relPathFromKey( $key );
				$oldPath = $repoOld->getZonePath( 'public' ) . '/' . $pathRel;
				if ( $repoOld->fileExists( $oldPath ) ) {
					$repoOld->getBackend()->delete( [ 'src' => $oldPath ] ); // delete!
				}
			}
		}

		# Start up the user's userpages if set to do so.
		# Will not append, so previous content will be blanked.
		$this->createUserPage( $user );

		# Greet the new user if set to do so.
		$this->createUserTalkPage( $user );
	}

	protected static function getGroupFromType( $type ) {
		global $wgAccountRequestTypes;

		$group = '';
		// Format is (type => (subpage par, group key, group text))
		if ( isset( $wgAccountRequestTypes[$type][1] ) ) {
			$group = $wgAccountRequestTypes[$type][1];
		}

		return $group;
	}

	protected static function getAutoTextFromType( $type ) {
		global $wgAccountRequestTypes;

		$groupText = '';
		// Format is (type => (subpage par, group key, group text))
		if ( isset( $wgAccountRequestTypes[$type][2] ) ) {
			$groupText = $wgAccountRequestTypes[$type][2];
		}

		return $groupText;
	}

	protected function createUserPage( User $user ) {
		global $wgMakeUserPageFromBio, $wgAutoUserBioText;
		global $wgConfirmAccountSortkey, $wgContLang;

		$body = ''; // page text

		if ( $wgMakeUserPageFromBio ) {
			# Add account request bio to userpage
			$body .= $this->bio;
			# Add any automatic text for all confirmed accounts
			if ( $wgAutoUserBioText != '' ) {
				$body .= "\n\n{$wgAutoUserBioText}";
			}
		}

		# Add any automatic text for confirmed accounts of this type
		$autoText = self::getAutoTextFromType( $this->type );
		if ( $autoText != '' ) {
			$body .= "\n\n{$autoText}";
		}

		# Add any areas of interest categories...
		foreach ( ConfirmAccount::getUserAreaConfig() as $name => $conf ) {
			if ( in_array( $name, $this->areas ) ) {
				# General userpage text for anyone with this interest
				if ( $conf['userText'] != '' ) {
					$body .= $conf['userText'];
				}
				# Message for users with this interested with the given account type
				if ( isset( $conf['grpUserText'][$this->type] )
					&& $conf['grpUserText'][$this->type] != ''
				) {
					$body .= $conf['grpUserText'];
				}
			}
		}

		# Set sortkey and use it on userpage. This can be used to
		# normalize things like firstname, lastname and so fourth.
		if ( !empty( $wgConfirmAccountSortkey ) ) {
			$sortKey = preg_replace(
				$wgConfirmAccountSortkey[0],
				$wgConfirmAccountSortkey[1],
				$user->getUserPage()->getText()
			);
			$body .= "\n{{DEFAULTSORT:{$sortKey}}}";
			# Clean up any other categories...
			$catNS = $wgContLang->getNSText( NS_CATEGORY );
			$replace = '/\[\[' . preg_quote( $catNS ) . ':([^\]]+)\]\]/i'; // [[Category:x]]
			$with = "[[{$catNS}:$1|" . str_replace( '$', '\$', $sortKey ) . "]]"; // [[Category:x|sortkey]]
			$body = preg_replace( $replace, $with, $body );
		}

		# Create userpage!
		if ( $body !== '' ) {
			$article = new WikiPage( $user->getUserPage() );
			$article->doEditContent(
				ContentHandler::makeContent( $body, $article->getTitle() ),
				wfMessage( 'confirmaccount-summary' )->inContentLanguage()->text(),
				EDIT_MINOR
			);
		}
	}

	protected function createUserTalkPage( User $user ) {
		global $wgAutoWelcomeNewUsers;

		if ( $wgAutoWelcomeNewUsers ) {
			$msgObj = wfMessage( "confirmaccount-welc-pos{$this->type}" );
			$welcome = $msgObj->isDisabled()
				? wfMessage( 'confirmaccount-welc' )->text()
				: $msgObj->text(); // custom message
			# Add user welcome message!
			$article = new WikiPage( $user->getTalkPage() );
			$article->doEditContent(
				ContentHandler::makeContent( "{$welcome} ~~~~", $article->getTitle() ),
				wfMessage( 'confirmaccount-wsum' )->inContentLanguage()->text(),
				EDIT_MINOR,
				false,
				$this->admin
			);
		}
	}
}
