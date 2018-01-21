<?php
/**
 * Special page for the  CategoryTree extension, an AJAX based gadget
 * to display the category structure of a wiki
 *
 * @file
 * @ingroup Extensions
 * @author Daniel Kinzler, brightbyte.de
 * @copyright © 2006 Daniel Kinzler
 * @license GNU General Public Licence 2.0 or later
 */

class CategoryTreePage extends SpecialPage {
	public $target = '';

	/**
	 * @var CategoryTree
	 */
	public $tree = null;

	function __construct() {
		parent::__construct( 'CategoryTree', '', true );
	}

	/**
	 * @param $name
	 * @return mixed
	 */
	function getOption( $name ) {
		global $wgCategoryTreeDefaultOptions;

		if ( $this->tree ) {
			return $this->tree->getOption( $name );
		} else {
			return $wgCategoryTreeDefaultOptions[$name];
		}
	}

	/**
	 * Main execution function
	 * @param $par array Parameters passed to the page
	 */
	function execute( $par ) {
		global $wgCategoryTreeDefaultOptions, $wgCategoryTreeSpecialPageOptions, $wgCategoryTreeForceHeaders;

		$this->setHeaders();
		$request = $this->getRequest();
		if ( $par ) {
			$this->target = $par;
		} else {
			$this->target = $request->getVal( 'target', $this->msg( 'rootcategory' )->text() );
		}

		$this->target = trim( $this->target );

		# HACK for undefined root category
		if ( $this->target == '<rootcategory>' || $this->target == '&lt;rootcategory&gt;' ) {
			$this->target = null;
		}

		$options = array();

		# grab all known options from the request. Normalization is done by the CategoryTree class
		foreach ( $wgCategoryTreeDefaultOptions as $option => $default ) {
			if ( isset( $wgCategoryTreeSpecialPageOptions[$option] ) ) {
				$default = $wgCategoryTreeSpecialPageOptions[$option];
			}

			$options[$option] = $request->getVal( $option, $default );
		}

		$this->tree = new CategoryTree( $options );

		$output = $this->getOutput();
		$output->addWikiMsg( 'categorytree-header' );

		$this->executeInputForm();

		if ( $this->target !== '' && $this->target !== null ) {
			if ( !$wgCategoryTreeForceHeaders ) {
				CategoryTree::setHeaders( $output );
			}

			$title = CategoryTree::makeTitle( $this->target );

			if ( $title && $title->getArticleID() ) {
				$output->addHTML( Xml::openElement( 'div', array( 'class' => 'CategoryTreeParents' ) ) );
				$output->addHTML( $this->msg( 'categorytree-parents' )->parse() );
				$output->addHTML( $this->msg( 'colon-separator' )->escaped() );

				$parents = $this->tree->renderParents( $title );

				if ( $parents == '' ) {
					$output->addHTML( $this->msg( 'categorytree-no-parent-categories' )->parse() );
				} else {
					$output->addHTML( $parents );
				}

				$output->addHTML( Xml::closeElement( 'div' ) );

				$output->addHTML( Xml::openElement( 'div', array( 'class' => 'CategoryTreeResult' ) ) );
				$output->addHTML( $this->tree->renderNode( $title, 1 ) );
				$output->addHTML( Xml::closeElement( 'div' ) );
			} else {
				$output->addHTML( Xml::openElement( 'div', array( 'class' => 'CategoryTreeNotice' ) ) );
				$output->addHTML( $this->msg( 'categorytree-not-found', $this->target )->parse() );
				$output->addHTML( Xml::closeElement( 'div' ) );
			}
		}
	}

	/**
	 * Input form for entering a category
	 */
	function executeInputForm() {
		$namespaces = $this->getRequest()->getVal( 'namespaces', '' );
		//mode may be overriden by namespaces option
		$mode = ( $namespaces == '' ? $this->getOption( 'mode' ) : CategoryTreeMode::ALL );
		if ( $mode == CategoryTreeMode::CATEGORIES ) {
			$modeDefault = 'categories';
		} elseif( $mode == CategoryTreeMode::PAGES ) {
			$modeDefault = 'pages';
		} else {
			$modeDefault = 'all';
		}

		$formDescriptor = [
			'category' => [
				'type' => 'title',
				'name' => 'target',
				'label-message' => 'categorytree-category',
				'namespace' => NS_CATEGORY,
			],

			'mode' => [
				'type' => 'select',
				'name' => 'mode',
				'label-message' => 'categorytree-mode-label',
				'options-messages' => [
					'categorytree-mode-categories' => 'categories',
					'categorytree-mode-pages' => 'pages',
					'categorytree-mode-all' => 'all',
				],
				'default' => $modeDefault,
				'nodata' => true,
			],

			'namespace' => [
				'type' => 'namespaceselect',
				'name' => 'namespaces',
				'label-message' => 'namespace',
				'all' => '',
			],
		];

		$form = HTMLForm::factory( 'ooui', $formDescriptor, $this->getContext() )
			->addHiddenFields( [ 'title' => $this->getPageTitle()->getPrefixedDbKey() ] )
			->setWrapperLegendMsg( 'categorytree-legend' )
			->setSubmitTextMsg( 'categorytree-go' )
			->setMethod( 'get' )
			->prepareForm()
			->displayForm( false );
	}

	/**
	 * Return an array of subpages beginning with $search that this special page will accept.
	 *
	 * @param string $search Prefix to search for
	 * @param int $limit Maximum number of results to return (usually 10)
	 * @param int $offset Number of results to skip (usually 0)
	 * @return string[] Matching subpages
	 */
	public function prefixSearchSubpages( $search, $limit, $offset ) {
		$title = Title::newFromText( $search, NS_CATEGORY );
		if ( $title && $title->getNamespace() !== NS_CATEGORY ) {
			// Someone searching for something like "Wikipedia:Foo"
			$title = Title::makeTitleSafe( NS_CATEGORY, $search );
		}
		if ( !$title ) {
			// No prefix suggestion outside of category namespace
			return array();
		}
		// Autocomplete subpage the same as a normal search, but just for categories
		$prefixSearcher = new TitlePrefixSearch;
		$result = $prefixSearcher->search( $title->getPrefixedText(), $limit, array( NS_CATEGORY ), $offset );

		return array_map( function ( Title $t ) {
			// Remove namespace in search suggestion
			return $t->getText();
		}, $result );
	}

	protected function getGroupName() {
		return 'pages';
	}

}
