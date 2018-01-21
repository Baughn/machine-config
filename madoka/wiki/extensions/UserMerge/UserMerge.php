<?php
if ( function_exists( 'wfLoadExtension' ) ) {
	wfLoadExtension( 'UserMerge' );
	// Keep i18n globals so mergeMessageFileList.php doesn't break
	$wgMessagesDirs['UserMerge'] = __DIR__ . '/i18n';
	$wgExtensionMessagesFiles['UserMergeAlias'] = __DIR__ . '/UserMerge.alias.php';
	/* wfWarn(
		'Deprecated PHP entry point used for UserMerge extension. Please use wfLoadExtension instead, ' .
		'see https://www.mediawiki.org/wiki/Extension_registration for more details.'
	); */
	return;
} else {
	die( 'This version of the UserMerge extension requires MediaWiki 1.25+' );
}
