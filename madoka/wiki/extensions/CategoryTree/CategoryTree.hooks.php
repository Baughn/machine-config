<?php
/**
 * Hooks for the CategoryTree extension, an AJAX based gadget
 * to display the category structure of a wiki
 *
 * @file
 * @ingroup Extensions
 * @author Daniel Kinzler, brightbyte.de
 * @copyright © 2006-2008 Daniel Kinzler and others
 * @license GNU General Public Licence 2.0 or later
 */

class CategoryTreeHooks {

	/**
	 * Adds hooks and adjusts the config once MediaWiki is fully initialised
	 */
	public static function initialize() {
		global $wgUseAjax, $wgHooks, $wgRequest;
		global $wgCategoryTreeDefaultOptions, $wgCategoryTreeDefaultMode, $wgCategoryTreeOmitNamespace;
		global $wgCategoryTreeCategoryPageOptions, $wgCategoryTreeCategoryPageMode, $wgCategoryTreeAllowTag;
		global $wgCategoryTreeSidebarRoot, $wgCategoryTreeForceHeaders, $wgCategoryTreeHijackPageCategories;

		# Abort if AJAX is not enabled
		if ( !$wgUseAjax ) {
			wfDebug( 'CategoryTreeHooks::initialize: $wgUseAjax is not enabled, aborting extension setup.' );
			return;
		}

		if ( $wgCategoryTreeSidebarRoot ) {
			$wgCategoryTreeForceHeaders = true; # needed on every page anyway
			$wgHooks['SkinTemplateOutputPageBeforeExec'][] = 'CategoryTreeHooks::skinTemplateOutputPageBeforeExec';
		}

		if ( $wgCategoryTreeHijackPageCategories ) {
			$wgCategoryTreeForceHeaders = true; # needed on almost every page anyway
			$wgHooks['OutputPageMakeCategoryLinks'][] = 'CategoryTreeHooks::outputPageMakeCategoryLinks';
			$wgHooks['SkinJoinCategoryLinks'][] = 'CategoryTreeHooks::skinJoinCategoryLinks';
		}

		if ( $wgCategoryTreeAllowTag ) {
			$wgHooks['ParserFirstCallInit'][] = 'CategoryTreeHooks::setHooks';
		}

		if ( !isset( $wgCategoryTreeDefaultOptions['mode'] ) || is_null( $wgCategoryTreeDefaultOptions['mode'] ) ) {
			$wgCategoryTreeDefaultOptions['mode'] = $wgCategoryTreeDefaultMode;
		}

		if ( !isset( $wgCategoryTreeDefaultOptions['hideprefix'] ) || is_null( $wgCategoryTreeDefaultOptions['hideprefix'] ) ) {
			$wgCategoryTreeDefaultOptions['hideprefix'] = $wgCategoryTreeOmitNamespace;
		}

		if ( !isset( $wgCategoryTreeCategoryPageOptions['mode'] ) || is_null( $wgCategoryTreeCategoryPageOptions['mode'] ) ) {
			$wgCategoryTreeCategoryPageOptions['mode'] = ( $mode = $wgRequest->getVal( 'mode' ) ) ? CategoryTree::decodeMode( $mode ) : $wgCategoryTreeCategoryPageMode;
		}

		if ( $wgCategoryTreeForceHeaders ) {
			$wgHooks['BeforePageDisplay'][] = 'CategoryTreeHooks::addHeaders';
		} else {
			$wgHooks['OutputPageParserOutput'][] = 'CategoryTreeHooks::parserOutput';
		}

		$wgHooks['MakeGlobalVariablesScript'][] = 'CategoryTreeHooks::getConfigVars';
	}

	/**
	 * @param $parser Parser
	 * @return bool
	 */
	public static function setHooks( $parser ) {
		$parser->setHook( 'categorytree' , 'CategoryTreeHooks::parserHook' );
		$parser->setFunctionHook( 'categorytree' , 'CategoryTreeHooks::parserFunction' );
		return true;
	}

	/**
	 * Entry point for the {{#categorytree}} tag parser function.
	 * This is a wrapper around CategoryTreeHooks::parserHook
	 * @param $parser Parser
	 * @return array|string
	 */
	public static function parserFunction( $parser ) {
		$params = func_get_args();
		array_shift( $params ); // first is $parser, strip it

		// first user-supplied parameter must be category name
		if ( !$params ) {
			return ''; // no category specified, return nothing
		}
		$cat = array_shift( $params );

		// build associative arguments from flat parameter list
		$argv = array();
		foreach ( $params as $p ) {
			if ( preg_match( '/^\s*(\S.*?)\s*=\s*(.*?)\s*$/', $p, $m ) ) {
				$k = $m[1];
				$v = preg_replace( '/^"\s*(.*?)\s*"$/', '$1', $m[2] ); // strip any quotes enclusing the value
			} else {
				$k = trim( $p );
				$v = true;
			}

			$argv[$k] = $v;
		}

		// now handle just like a <categorytree> tag
		$html = self::parserHook( $cat, $argv, $parser );
		return array( $html, 'noparse' => true, 'isHTML' => true );
	}

	/**
	 * Hook implementation for injecting a category tree into the sidebar.
	 * Registered automatically if $wgCategoryTreeSidebarRoot is set to a category name.
	 * @param $skin
	 * @param $tpl SkinTemplate
	 * @return bool
	 */
	public static function skinTemplateOutputPageBeforeExec( $skin, $tpl ) {
		global $wgCategoryTreeSidebarRoot, $wgCategoryTreeSidebarOptions;

		$html = self::parserHook( $wgCategoryTreeSidebarRoot, $wgCategoryTreeSidebarOptions );
		if ( $html ) {
			$tpl->data['sidebar']['categorytree-portlet'] = $html;
		}

		return true;
	}

	/**
	 * Entry point for the <categorytree> tag parser hook.
	 * This loads CategoryTreeFunctions.php and calls CategoryTree::getTag()
	 * @param $cat
	 * @param $argv
	 * @param $parser Parser
	 * @param $allowMissing bool
	 * @return bool|string
	 */
	public static function parserHook( $cat, $argv, $parser = null, $allowMissing = false ) {
		global $wgOut;

		if ( $parser ) {
			$parser->mOutput->mCategoryTreeTag = true; # flag for use by CategoryTreeHooks::parserOutput
		} else {
			CategoryTree::setHeaders( $wgOut );
		}

		$ct = new CategoryTree( $argv );

		$attr = Sanitizer::validateTagAttributes( $argv, 'div' );

		$hideroot = isset( $argv[ 'hideroot' ] ) ? CategoryTree::decodeBoolean( $argv[ 'hideroot' ] ) : null;
		$onlyroot = isset( $argv[ 'onlyroot' ] ) ? CategoryTree::decodeBoolean( $argv[ 'onlyroot' ] ) : null;
		$depthArg = isset( $argv[ 'depth' ] ) ? (int)$argv[ 'depth' ] : null;

		$depth = CategoryTree::capDepth( $ct->getOption( 'mode' ), $depthArg );
		if ( $onlyroot ) {
			$depth = 0;
		}

		return $ct->getTag( $parser, $cat, $hideroot, $attr, $depth, $allowMissing );
	}

	/**
	 * Hook callback that injects messages and things into the <head> tag,
	 * if needed in the current page.
	 * Does nothing if $parserOutput->mCategoryTreeTag is not set
	 * @param $outputPage OutputPage
	 * @param $parserOutput ParserOutput
	 * @return bool
	 */
	public static function parserOutput( $outputPage, $parserOutput )  {
		if ( !empty( $parserOutput->mCategoryTreeTag ) ) {
			CategoryTree::setHeaders( $outputPage );
		}
		return true;
	}

	/**
	 * BeforePageDisplay hook. This hook is set when $wgCategoryTreeForceHeaders
	 * is set.
	 * Otherwise similar to CategoryTreeHooks::parserOutput.
	 * @param $out OutputPage
	 * @param $skin Skin
	 * @return bool
	 */
	public static function addHeaders( OutputPage $out, Skin $skin )  {
		CategoryTree::setHeaders( $out );
		return true;
	}

	/**
	 * ArticleFromTitle hook, override category page handling
	 *
	 * @param $title Title
	 * @param $article Article
	 * @return bool
	 */
	public static function articleFromTitle( $title, &$article ) {
		if ( $title->getNamespace() == NS_CATEGORY ) {
			$article = new CategoryTreeCategoryPage( $title );
		}
		return true;
	}

	/**
	 * OutputPageMakeCategoryLinks hook, override category links
	 * @param $out
	 * @param $categories
	 * @param $links
	 * @return bool
	 */
	public static function outputPageMakeCategoryLinks( $out, &$categories, &$links ) {
		global $wgCategoryTreePageCategoryOptions;

		foreach ( $categories as $category => $type ) {
			$links[$type][] = self::parserHook( $category, $wgCategoryTreePageCategoryOptions, null, true );
		}

		return false;
	}

	/**
	 * @param $skin
	 * @param $links
	 * @param $result
	 * @return bool
	 */
	public static function skinJoinCategoryLinks( $skin, &$links, &$result ) {
		$embed = '<div class="CategoryTreeCategoryBarItem">';
		$pop = '</div>';
		$sep = ' ';

		$result = $embed . implode ( "{$pop} {$sep} {$embed}" , $links ) . $pop;

		return false;
	}

	/**
	 * @param $vars
	 * @return bool
	 */
	public static function getConfigVars( &$vars ) {
		global $wgCategoryTreeCategoryPageOptions;

		// Look this is pretty bad but Category tree is just whacky, it needs to be rewritten
		$ct = new CategoryTree( $wgCategoryTreeCategoryPageOptions );
		$vars['wgCategoryTreePageCategoryOptions'] = $ct->getOptionsAsJsStructure();
		return true;
	}
}
