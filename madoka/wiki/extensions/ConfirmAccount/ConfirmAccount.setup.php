<?php
/**
 * Class containing basic setup functions.
 */
class ConfirmAccountSetup {
	/**
	 * Register source code paths.
	 * This function must NOT depend on any config vars.
	 *
	 * @param $classes Array $classes
	 * @param $messagesDirs Array $messagesDirs
	 * @param $messagesFiles Array $messagesFiles
	 * @return void
	 */
	public static function defineSourcePaths(
		array &$classes, array &$messagesDirs, array &$messagesFiles
	) {
		$dir = __DIR__;

		# Basic directory layout
		$backendDir       = "$dir/backend";
		$schemaDir        = "$dir/backend/schema";
		$businessDir      = "$dir/business";
		$frontendDir      = "$dir/frontend";
		$langDir          = "$dir/frontend/language";
		$spActionDir      = "$dir/frontend/specialpages/actions";

		# Main i18n file and special page alias file
		$messagesDirs['ConfirmAccount'] = __DIR__ . '/i18n/core';
		$messagesFiles['ConfirmAccountAliases'] = "$langDir/ConfirmAccount.alias.php";

		# UI setup class
		$classes['ConfirmAccountUISetup'] = "$frontendDir/ConfirmAccountUI.setup.php";
		# UI event handler classes
		$classes['ConfirmAccountUIHooks'] = "$frontendDir/ConfirmAccountUI.hooks.php";

		# UI to request an account
		$classes['RequestAccountPage'] = "$spActionDir/RequestAccount_body.php";
		$messagesDirs['RequestAccountPage'] = __DIR__ . '/i18n/requestaccount';
		# UI to confirm accounts
		$classes['ConfirmAccountsPage'] = "$spActionDir/ConfirmAccount_body.php";
		$classes['ConfirmAccountsPager'] = "$spActionDir/ConfirmAccount_body.php";
		$messagesDirs['ConfirmAccountPage'] = __DIR__ . '/i18n/confirmaccount';
		# UI to see account credentials
		$classes['UserCredentialsPage'] = "$spActionDir/UserCredentials_body.php";
		$messagesDirs['UserCredentialsPage'] = __DIR__ . '/i18n/usercredentials';

		# Utility functions
		$classes['ConfirmAccount'] = "$backendDir/ConfirmAccount.class.php";
		# Data access objects
		$classes['UserAccountRequest'] = "$backendDir/UserAccountRequest.php";

		# Business logic
		$classes['AccountRequestSubmission'] = "$businessDir/AccountRequestSubmission.php";
		$classes['AccountConfirmSubmission'] = "$businessDir/AccountConfirmSubmission.php";
		$classes['ConfirmAccountPreAuthenticationProvider'] =
			"$businessDir/ConfirmAccountPreAuthenticationProvider.php";

		# Schema changes
		$classes['ConfirmAccountUpdaterHooks'] = "$schemaDir/ConfirmAccountUpdater.hooks.php";
	}
}
