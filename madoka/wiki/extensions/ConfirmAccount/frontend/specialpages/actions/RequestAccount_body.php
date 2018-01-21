<?php

class RequestAccountPage extends SpecialPage {
	protected $mUsername; // string
	protected $mRealName; // string
	protected $mEmail; // string
	protected $mBio; // string
	protected $mNotes; // string
	protected $mUrls; // string
	protected $mToS; // bool
	protected $mType; // integer
	/** @var Array */
	protected $mAreas;

	protected $mPrevAttachment; // string
	protected $mForgotAttachment; // bool
	protected $mSrcName; // string
	protected $mFileSize; // integer
	protected $mTempPath; // string

	function __construct() {
		parent::__construct( 'RequestAccount' );
	}

	public function doesWrites() {
		return true;
	}

	function execute( $par ) {
		global $wgAccountRequestTypes;

		$reqUser = $this->getUser();
		$request = $this->getRequest();

		$block = ConfirmAccount::getAccountRequestBlock( $reqUser );
		if ( $block ) {
			throw new UserBlockedError( $block );
		} elseif ( wfReadOnly() ) {
			throw new ReadOnlyError();
		}

		$this->setHeaders();

		$this->mRealName = trim( $request->getText( 'wpRealName' ) );
		# We may only want real names being used
		$this->mUsername = !$this->hasItem( 'UserName' )
			? $this->mRealName
			: $request->getText( 'wpUsername' );
		$this->mUsername = trim( $this->mUsername );
		# CV/resume attachment...
		if ( $this->hasItem( 'CV' ) ) {
			$this->initializeUpload( $request );
			$this->mPrevAttachment = $request->getText( 'attachment' );
			$this->mForgotAttachment = $request->getBool( 'forgotAttachment' );
		}
		# Other identifying fields...
		$this->mEmail = trim( $request->getText( 'wpEmail' ) );
		$this->mBio = $this->hasItem( 'Biography' ) ? $request->getText( 'wpBio', '' ) : '';
		$this->mNotes = $this->hasItem( 'Notes' ) ? $request->getText( 'wpNotes', '' ) : '';
		$this->mUrls = $this->hasItem( 'Links' ) ? $request->getText( 'wpUrls', '' ) : '';
		# Site terms of service...
		$this->mToS = $this->hasItem( 'TermsOfService' ) ? $request->getBool( 'wpToS' ) : false;
		# Which account request queue this belongs in...
		$this->mType = $request->getInt( 'wpType' );
		$this->mType = isset( $wgAccountRequestTypes[$this->mType] ) ? $this->mType : 0;
		# Load areas user plans to be active in...
		$this->mAreas = [];
		if ( $this->hasItem( 'AreasOfInterest' ) ) {
			foreach ( ConfirmAccount::getUserAreaConfig() as $name => $conf ) {
				$formName = "wpArea-" . htmlspecialchars( str_replace( ' ', '_', $name ) );
				$this->mAreas[$name] = $request->getInt( $formName, -1 );
			}
		}
		# We may be confirming an email address here
		$emailCode = $request->getText( 'wpEmailToken' );

		$action = $request->getVal( 'action' );
		if ( $request->wasPosted()
			&& $reqUser->matchEditToken( $request->getVal( 'wpEditToken' ) ) )
		{
			$this->mPrevAttachment = $this->mPrevAttachment
				? $this->mPrevAttachment
				: $this->mSrcName;
			$this->doSubmit();
		} elseif ( $action == 'confirmemail' ) {
			$this->confirmEmailToken( $emailCode );
		} else {
			$this->showForm();
		}

		$this->getOutput()->addModules( 'ext.confirmAccount' ); // CSS
	}

	protected function showForm( $msg = '', $forgotFile = 0 ) {
		global $wgAccountRequestTypes, $wgMakeUserPageFromBio;

		$reqUser = $this->getUser();

		$this->mForgotAttachment = $forgotFile;

		$out = $this->getOutput();
		$out->setPagetitle( $this->msg( "requestaccount" )->escaped() );
		# Output failure message if any
		if ( $msg ) {
			$out->addHTML( '<div class="errorbox">' . $msg . '</div><div class="visualClear"></div>' );
		}
		# Give notice to users that are logged in
		if ( $reqUser->getID() ) {
			$out->addWikiMsg( 'requestaccount-dup' );
		}

		$out->addWikiMsg( 'requestaccount-text' );

		$form = Xml::openElement( 'form', [ 'method' => 'post', 'name' => 'accountrequest',
			'action' => $this->getPageTitle()->getLocalUrl(), 'enctype' => 'multipart/form-data' ] );

		$form .= '<fieldset><legend>' . $this->msg( 'requestaccount-leg-user' )->escaped() . '</legend>';
		$form .= $this->msg( 'requestaccount-acc-text' )->parseAsBlock() . "\n";
		$form .= '<table style="padding:4px;">';
		if ( $this->hasItem( 'UserName' ) ) {
			$form .= "<tr><td>" . Xml::label( $this->msg( 'username' )->text(), 'wpUsername' ) . "</td>";
			$form .= "<td>" . Xml::input(
				'wpUsername', 30, $this->mUsername, [ 'id' => 'wpUsername' ]
			) . "</td></tr>\n";
		} else {
			$form .= "<tr><td>" . $this->msg( 'username' )->escaped() . "</td>";
			$form .= "<td>" . $this->msg( 'requestaccount-same' )->escaped() . "</td></tr>\n";
		}
		$form .= "<tr><td>" . Xml::label(
			$this->msg( 'requestaccount-email' )->text(), 'wpEmail'
		) . "</td>";
		$form .= "<td>" . Xml::input(
			'wpEmail', 30, $this->mEmail, [ 'id' => 'wpEmail' ]
		) . "</td></tr>\n";
		if ( count( $wgAccountRequestTypes ) > 1 ) {
			$form .= "<tr><td>" . $this->msg( 'requestaccount-reqtype' )->escaped() . "</td><td>";
			$options = [];
			foreach ( $wgAccountRequestTypes as $i => $params ) {
				// Give grep a chance to find the usages: requestaccount-level-0, requestaccount-level-1
				$options[] = Xml::option(
					$this->msg( "requestaccount-level-$i" )->text(), $i, ( $i == $this->mType )
				);
			}
			$form .= Xml::openElement( 'select', [ 'name' => "wpType" ] );
			$form .= implode( "\n", $options );
			$form .= Xml::closeElement( 'select' ) . "\n";
			$form .= '</td></tr>';
		}
		$form .= '</table></fieldset>';

		$userAreas = ConfirmAccount::getUserAreaConfig();
		if ( $this->hasItem( 'AreasOfInterest' ) && count( $userAreas ) > 0 ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'requestaccount-leg-areas' )->escaped() . '</legend>';
			$form .=  $this->msg( 'requestaccount-areas-text' )->parseAsBlock() . "\n";

			$form .= "<div style='height:150px; overflow:scroll; background-color:#f9f9f9;'>";
			$form .= "<table style='border-spacing: 5px; padding: 0px; background-color: #f9f9f9;'>
			<tr valign='top'>";
			$count = 0;
			foreach ( $userAreas as $name => $conf ) {
				$count++;
				if ( $count > 5 ) {
					$form .= "</tr><tr style='vertical-align:top;'>";
					$count = 1;
				}
				$formName = "wpArea-" . htmlspecialchars( str_replace( ' ', '_', $name ) );
				if ( $conf['project'] != '' ) {
					$linkRenderer = $this->getLinkRenderer();
					$pg = $linkRenderer->makeLink(
						Title::newFromText( $conf['project'] ),
						$this->msg( 'requestaccount-info' )->text()
					);
				} else {
					$pg = '';
				}
				$form .= "<td>" .
					Xml::checkLabel( $name, $formName, $formName, $this->mAreas[$name] > 0 ) .
					" {$pg}</td>\n";
			}
			$form .= "</tr></table></div>";
			$form .= '</fieldset>';
		}

		if ( $this->hasItem( 'Biography' ) || $this->hasItem( 'RealName' ) ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'requestaccount-leg-person' )->escaped() . '</legend>';
			if ( $this->hasItem( 'RealName' ) ) {
				$form .= '<table style="padding:4px;">';
				$form .= "<tr><td>" . Xml::label(
					$this->msg( 'requestaccount-real' )->text(), 'wpRealName'
				) . "</td>";
				$form .= "<td>" . Xml::input(
					'wpRealName', 35, $this->mRealName, [ 'id' => 'wpRealName' ]
				) . "</td></tr>\n";
				$form .= '</table>';
			}
			if ( $this->hasItem( 'Biography' ) ) {
				if ( $wgMakeUserPageFromBio ) {
					$form .= $this->msg( 'requestaccount-bio-text-i' )->parseAsBlock() . "\n";
				}
				$form .= $this->msg( 'requestaccount-bio-text' )->parseAsBlock() . "\n";
				$form .= "<p>" . $this->msg( 'requestaccount-bio' )->parse() . "\n";
				$form .= "<textarea tabindex='1' name='wpBio' id='wpBio' rows='12' cols='80'
				style='width: 100%; background-color: #f9f9f9;'>" .
					htmlspecialchars( $this->mBio ) . "</textarea></p>\n";
			}
			$form .= '</fieldset>';
		}

		if ( $this->hasItem( 'CV' ) || $this->hasItem( 'Notes' ) || $this->hasItem( 'Links' ) ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'requestaccount-leg-other' )->escaped() . '</legend>';
			$form .= $this->msg( 'requestaccount-ext-text' )->parseAsBlock() . "\n";
			if ( $this->hasItem( 'CV' ) ) {
				$form .= "<p>" . $this->msg( 'requestaccount-attach' )->escaped() . " ";
				$form .= Xml::input( 'wpUploadFile', 35, '',
					[ 'id' => 'wpUploadFile', 'type' => 'file' ] ) . "</p>\n";
			}
			if ( $this->hasItem( 'Notes' ) ) {
				$form .= "<p>" . $this->msg( 'requestaccount-notes' )->escaped() . "\n";
				$form .= "<textarea tabindex='1' name='wpNotes' id='wpNotes' rows='3' cols='80'
				style='width: 100%; background-color: #f9f9f9;'>" .
					htmlspecialchars( $this->mNotes ) .
					"</textarea></p>\n";
			}
			if ( $this->hasItem( 'Links' ) ) {
				$form .= "<p>" . $this->msg( 'requestaccount-urls' )->escaped() . "\n";
				$form .= "<textarea tabindex='1' name='wpUrls' id='wpUrls' rows='2' cols='80'
				style='width: 100%; background-color: #f9f9f9;'>" .
					htmlspecialchars( $this->mUrls ) .
					"</textarea></p>\n";
			}
			$form .= '</fieldset>';
		}

		if ( $this->hasItem( 'TermsOfService' ) ) {
			$form .= '<fieldset>';
			$form .= '<legend>' . $this->msg( 'requestaccount-leg-tos' )->escaped() . '</legend>';
			$form .= "<p>" . Xml::check( 'wpToS', $this->mToS, [ 'id' => 'wpToS' ] ) .
				' <label for="wpToS">' . $this->msg( 'requestaccount-tos' )->parse() . "</label></p>\n";
			$form .= '</fieldset>';
		}

		# FIXME: do this better...
		global $wgConfirmAccountCaptchas, $wgCaptchaClass, $wgCaptchaTriggers;
		if ( $wgConfirmAccountCaptchas && isset( $wgCaptchaClass )
			&& $wgCaptchaTriggers['createaccount'] && !$reqUser->isAllowed( 'skipcaptcha' ) )
		{
			$captcha = new $wgCaptchaClass;

			$formInformation = $captcha->getFormInformation();
			$formMetainfo = $formInformation;
			unset( $formMetainfo['html'] );
			$captcha->addFormInformationToOutput( $out, $formMetainfo );

			# Hook point to add captchas
			$form .= '<fieldset>';
			$form .= $this->msg( 'captcha-createaccount' )->parseAsBlock();
			$form .= $formInformation['html'];
			$form .= '</fieldset>';
		}
		$form .= Html::Hidden( 'title', $this->getPageTitle()->getPrefixedDBKey() ) . "\n";
		$form .= Html::Hidden( 'wpEditToken', $reqUser->getEditToken() ) . "\n";
		$form .= Html::Hidden( 'attachment', $this->mPrevAttachment ) . "\n";
		$form .= Html::Hidden( 'forgotAttachment', $this->mForgotAttachment ) . "\n";
		$form .= "<p>" . Xml::submitButton( $this->msg( 'requestaccount-submit' )->text() ) . "</p>";
		$form .= Xml::closeElement( 'form' );

		$out->addHTML( $form );

		$out->addWikiMsg( 'requestaccount-footer' );
	}

	protected function hasItem( $name ) {
		global $wgConfirmAccountRequestFormItems;

		return $wgConfirmAccountRequestFormItems[$name]['enabled'];
	}

	protected function doSubmit() {
		# Now create a dummy user ($u) and check if it is valid
		$name = trim( $this->mUsername );
		$u = User::newFromName( $name, 'creatable' );
		if ( !$u ) {
			$this->showForm( $this->msg( 'noname' )->escaped() );
			return;
		}
		# Set some additional data so the AbortNewAccount hook can be
		# used for more than just username validation
		$u->setEmail( $this->mEmail );
		$u->setRealName( $this->mRealName );
		# FIXME: Hack! If we don't want captchas for requests, temporarily turn it off!
		global $wgConfirmAccountCaptchas, $wgCaptchaTriggers;
		if ( !$wgConfirmAccountCaptchas && isset( $wgCaptchaTriggers ) ) {
			$old = $wgCaptchaTriggers['createaccount'];
			$wgCaptchaTriggers['createaccount'] = false;
		}
		$abortError = '';
		if ( !Hooks::run( 'AbortNewAccount', [ $u, &$abortError ] ) ) {
			// Hook point to add extra creation throttles and blocks
			wfDebug( "RequestAccount::doSubmit: a hook blocked creation\n" );
			$this->showForm( $abortError );
			return;
		}
		# Set it back!
		if ( !$wgConfirmAccountCaptchas && isset( $wgCaptchaTriggers ) ) {
			$wgCaptchaTriggers['createaccount'] = $old;
		}

		# Build submission object...
		$areaSet = []; // make a simple list of interests
		foreach ( $this->mAreas as $area => $val ) {
			if ( $val > 0 ) {
				$areaSet[] = $area;
			}
		}

		$submission = new AccountRequestSubmission(
			$this->getUser(),
			[
				'userName'                  => $name,
				'realName'                  => $this->mRealName,
				'tosAccepted'               => $this->mToS,
				'email'                     => $this->mEmail,
				'bio'                       => $this->mBio,
				'notes'                     => $this->mNotes,
				'urls'                      => $this->mUrls,
				'type'                      => $this->mType,
				'areas'                     => $areaSet,
				'registration'              => wfTimestampNow(),
				'ip'                        => $this->getRequest()->getIP(),
				'xff'                       => $this->getRequest()->getHeader( 'X-Forwarded-For' ),
				'agent'                     => $this->getRequest()->getHeader( 'User-Agent' ),
				'attachmentPrevName'        => $this->mPrevAttachment,
				'attachmentSrcName'         => $this->mSrcName,
				'attachmentDidNotForget'    => $this->mForgotAttachment, // confusing name :)
				'attachmentSize'            => $this->mFileSize,
				'attachmentTempPath'        => $this->mTempPath
			]
		);

		# Actually submit!
		list( $status, $msg ) = $submission->submit( $this->getContext() );
		# Account for state changes
		$this->mForgotAttachment = $submission->getAttachmentDidNotForget();
		$this->mPrevAttachment = $submission->getAttachtmentPrevName();
		# Check for error messages
		if ( $status !== true ) {
			$this->showForm( $msg );
			return;
		}

		# Done!
		$this->showSuccess();
	}

	protected function showSuccess() {
		$out = $this->getOutput();
		$out->setPagetitle( $this->msg( "requestaccount" )->escaped() );
		$out->addWikiMsg( 'requestaccount-sent' );
		$out->returnToMain();
	}

	/**
	 * Initialize the uploaded file from PHP data
	 * @param $request WebRequest
	 */
	protected function initializeUpload( $request ) {
		$file = new WebRequestUpload( $request, 'wpUploadFile' );
		$this->mTempPath = $file->getTempName();
		$this->mFileSize = $file->getSize();
		$this->mSrcName  = $file->getName();
	}

	/**
	 * (a) Try to confirm an email address via a token
	 * (b) Notify $wgConfirmAccountContact on success
	 * @param $code string The token
	 * @return void
	 */
	protected function confirmEmailToken( $code ) {
		global $wgConfirmAccountContact, $wgPasswordSender;

		$reqUser = $this->getUser();
		$out = $this->getOutput();
		# Confirm if this token is in the pending requests
		$name = ConfirmAccount::requestNameFromEmailToken( $code );
		if ( $name !== false ) {
			# Send confirmation email to prospective user
			ConfirmAccount::confirmEmail( $name );

			$adminsNotify = ConfirmAccount::getAdminsToNotify();
			# Send an email to admin after email has been confirmed
			if ( $adminsNotify->count() || $wgConfirmAccountContact != '' ) {
				$title = SpecialPage::getTitleFor( 'ConfirmAccounts' );
				$subject = $this->msg(
					'requestaccount-email-subj-admin' )->inContentLanguage()->escaped();
				$body = $this->msg(
					'requestaccount-email-body-admin', $name )->params(
						$title->getCanonicalURL() )->inContentLanguage()->text();
				# Actually send the email...
				if ( $wgConfirmAccountContact != '' ) {
					$source = new MailAddress( $wgPasswordSender, wfMessage( 'emailsender' )->text() );
					$target = new MailAddress( $wgConfirmAccountContact );
					$result = UserMailer::send( $target, $source, $subject, $body );
					if ( !$result->isOK() ) {
						wfDebug( "Could not sent email to admin at $target\n" );
					}
				}
				# Send an email to all users with "confirmaccount-notify" rights
				foreach ( $adminsNotify as $adminNotify ) {
					if ( $adminNotify->canReceiveEmail() ) {
						$adminNotify->sendMail( $subject, $body );
					}
				}
			}
			$out->addWikiMsg( 'requestaccount-econf' );
			$out->returnToMain();
		} else {
			# Maybe the user confirmed after account was created...
			$user = User::newFromConfirmationCode( $code, User::READ_LATEST );
			if ( is_object( $user ) ) {
				$user->confirmEmail();
				$user->saveSettings();
				$message = $reqUser->isLoggedIn()
					? 'confirmemail_loggedin'
					: 'confirmemail_success';
				$out->addWikiMsg( $message );
				if ( !$reqUser->isLoggedIn() ) {
					$title = SpecialPage::getTitleFor( 'Userlogin' );
					$out->returnToMain( true, $title );
				}
			} else {
				$out->addWikiMsg( 'confirmemail_invalid' );
			}
		}
	}

	protected function getGroupName() {
		return 'login';
	}
}
