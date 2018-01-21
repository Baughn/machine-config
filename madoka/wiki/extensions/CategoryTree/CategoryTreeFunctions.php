<?php
/**
 * Core functions for the CategoryTree extension, an AJAX based gadget
 * to display the category structure of a wiki
 *
 * @file
 * @ingroup Extensions
 * @author Daniel Kinzler, brightbyte.de
 * @copyright © 2006-2007 Daniel Kinzler
 * @license GNU General Public Licence 2.0 or later
 */

class CategoryTree {
	public $mOptions = array();

	/**
	 * @param $options array
	 */
	function __construct( $options ) {
		global $wgCategoryTreeDefaultOptions;

		# ensure default values and order of options. Order may become important, it may influence the cache key!
		foreach ( $wgCategoryTreeDefaultOptions as $option => $default ) {
			if ( isset( $options[$option] ) && !is_null( $options[$option] ) ) {
				$this->mOptions[$option] = $options[$option];
			} else {
				$this->mOptions[$option] = $default;
			}
		}

		$this->mOptions['mode'] = self::decodeMode( $this->mOptions['mode'] );

		if ( $this->mOptions['mode'] == CategoryTreeMode::PARENTS ) {
			 $this->mOptions['namespaces'] = false; # namespace filter makes no sense with CategoryTreeMode::PARENTS
		}

		$this->mOptions['hideprefix'] = self::decodeHidePrefix( $this->mOptions['hideprefix'] );
		$this->mOptions['showcount']  = self::decodeBoolean( $this->mOptions['showcount'] );
		$this->mOptions['namespaces']  = self::decodeNamespaces( $this->mOptions['namespaces'] );

		if ( $this->mOptions['namespaces'] ) {
			# automatically adjust mode to match namespace filter
			if ( sizeof( $this->mOptions['namespaces'] ) === 1
				&& $this->mOptions['namespaces'][0] == NS_CATEGORY ) {
				$this->mOptions['mode'] = CategoryTreeMode::CATEGORIES;
			} elseif ( !in_array( NS_FILE, $this->mOptions['namespaces'] ) ) {
				$this->mOptions['mode'] = CategoryTreeMode::PAGES;
			} else {
				$this->mOptions['mode'] = CategoryTreeMode::ALL;
			}
		}
	}

	/**
	 * @param $name string
	 * @return mixed
	 */
	function getOption( $name ) {
		return $this->mOptions[$name];
	}

	/**
	 * @return bool
	 */
	function isInverse( ) {
		return $this->getOption( 'mode' ) == CategoryTreeMode::PARENTS;
	}

	/**
	 * @param $nn
	 * @return array|bool
	 */
	static function decodeNamespaces( $nn ) {
		global $wgContLang;

		if ( $nn === false || is_null( $nn ) ) {
			return false;
		}

		if ( !is_array( $nn ) ) {
			$nn = preg_split( '![\s#:|]+!', $nn );
		}

		$namespaces = array();

		foreach ( $nn as $n ) {
			if ( is_int( $n ) ) {
				$ns = $n;
			} else {
				$n = trim( $n );
				if ( $n === '' ) {
					continue;
				}

				$lower = strtolower( $n );

				if ( is_numeric( $n ) ) {
					$ns = (int)$n;
				} elseif ( $n == '-' || $n == '_' || $n == '*' || $lower == 'main' ) {
					$ns = NS_MAIN;
				} else {
					$ns = $wgContLang->getNsIndex( $n );
				}
			}

			if ( is_int( $ns ) ) {
				$namespaces[] = $ns;
			}
		}

		sort( $namespaces ); # get elements into canonical order
		return $namespaces;
	}

	/**
	 * @param $mode
	 * @return int|string
	 */
	static function decodeMode( $mode ) {
		global $wgCategoryTreeDefaultOptions;

		if ( is_null( $mode ) ) {
			return $wgCategoryTreeDefaultOptions['mode'];
		}
		if ( is_int( $mode ) ) {
			return $mode;
		}

		$mode = trim( strtolower( $mode ) );

		if ( is_numeric( $mode ) ) {
			return (int)$mode;
		}

		if ( $mode == 'all' ) {
			$mode = CategoryTreeMode::ALL;
		} elseif ( $mode == 'pages' ) {
			$mode = CategoryTreeMode::PAGES;
		} elseif ( $mode == 'categories' || $mode == 'sub' ) {
			$mode = CategoryTreeMode::CATEGORIES;
		} elseif ( $mode == 'parents' || $mode == 'super' || $mode == 'inverse' ) {
			$mode = CategoryTreeMode::PARENTS;
		} elseif ( $mode == 'default' ) {
			$mode = $wgCategoryTreeDefaultOptions['mode'];
		}

		return (int)$mode;
	}

	/**
	 * Helper function to convert a string to a boolean value.
	 * Perhaps make this a global function in MediaWiki proper
	 * @param $value
	 * @return bool|null|string
	 */
	static function decodeBoolean( $value ) {
		if ( is_null( $value ) ) {
			return null;
		}
		if ( is_bool( $value ) ) {
			return $value;
		}
		if ( is_int( $value ) ) {
			return ( $value > 0 );
		}

		$value = trim( strtolower( $value ) );
		if ( is_numeric( $value ) ) {
			return ( (int)$value > 0 );
		}

		if ( $value == 'yes' || $value == 'y' || $value == 'true' || $value == 't' || $value == 'on' ) {
			return true;
		} elseif ( $value == 'no' || $value == 'n' || $value == 'false' || $value == 'f' || $value == 'off' ) {
			return false;
		} elseif ( $value == 'null' || $value == 'default' || $value == 'none' || $value == 'x' ) {
			return null;
		} else {
			return false;
		}
	}

	/**
	 * @param $value
	 * @return int|string
	 */
	static function decodeHidePrefix( $value ) {
		global $wgCategoryTreeDefaultOptions;

		if ( is_null( $value ) ) {
			return $wgCategoryTreeDefaultOptions['hideprefix'];
		}
		if ( is_int( $value ) ) {
			return $value;
		}
		if ( $value === true ) {
			return CategoryTreeHidePrefix::ALWAYS;
		}
		if ( $value === false ) {
			return CategoryTreeHidePrefix::NEVER;
		}

		$value = trim( strtolower( $value ) );

		if ( $value == 'yes' || $value == 'y' || $value == 'true' || $value == 't' || $value == 'on' ) {
			return CategoryTreeHidePrefix::ALWAYS;
		} elseif ( $value == 'no' || $value == 'n' || $value == 'false' || $value == 'f' || $value == 'off' ) {
			return CategoryTreeHidePrefix::NEVER;
		} elseif ( $value == 'always' ) {
			return CategoryTreeHidePrefix::ALWAYS;
		} elseif ( $value == 'never' ) {
			return CategoryTreeHidePrefix::NEVER;
		} elseif ( $value == 'auto' ) {
			return CategoryTreeHidePrefix::AUTO;
		} elseif ( $value == 'categories' || $value == 'category' || $value == 'smart' ) {
			return CategoryTreeHidePrefix::CATEGORIES;
		} else {
			return $wgCategoryTreeDefaultOptions['hideprefix'];
		}
	}

	/**
	 * Add ResourceLoader modules to the OutputPage object
	 * @param OutputPage $outputPage
	 */
	static function setHeaders( $outputPage ) {
		# Add the modules
		$outputPage->addModuleStyles( 'ext.categoryTree.css' );
		$outputPage->addModules( 'ext.categoryTree' );
	}

	/**
	 * @param $options
	 * @param $enc
	 * @return mixed
	 * @throws Exception
	 */
	static function encodeOptions( $options, $enc ) {
		if ( $enc == 'mode' || $enc == '' ) {
			$opt = $options['mode'];
		} elseif ( $enc == 'json' ) {
			$opt = FormatJson::encode( $options );
		} else {
			throw new Exception( 'Unknown encoding for CategoryTree options: ' . $enc );
		}

		return $opt;
	}

	/**
	 * @param $depth null
	 * @return string
	 */
	function getOptionsAsCacheKey( $depth = null ) {
		$key = "";

		foreach ( $this->mOptions as $k => $v ) {
			if ( is_array( $v ) ) $v = implode( '|', $v );
			$key .= $k . ':' . $v . ';';
		}

		if ( !is_null( $depth ) ) {
			$key .= ";depth=" . $depth;
		}
		return $key;
	}

	/**
	 * @param $depth int|null
	 * @return mixed
	 */
	function getOptionsAsJsStructure( $depth = null ) {
		if ( !is_null( $depth ) ) {
			$opt = $this->mOptions;
			$opt['depth'] = $depth;
			$s = self::encodeOptions( $opt, 'json' );
		} else {
			$s = self::encodeOptions( $this->mOptions, 'json' );
		}

		return $s;
	}

	/**
	 * @return string
	 */
	function getOptionsAsUrlParameters() {
		return http_build_query( $this->mOptions );
	}

	/**
	 * Custom tag implementation. This is called by CategoryTreeHooks::parserHook, which is used to
	 * load CategoryTreeFunctions.php on demand.
	 * @param $parser Parser
	 * @param $category
	 * @param $hideroot bool
	 * @param $attr
	 * @param $depth int
	 * @param $allowMissing bool
	 * @return bool|string
	 */
	function getTag( $parser, $category, $hideroot = false, $attr, $depth = 1, $allowMissing = false ) {
		global $wgCategoryTreeDisableCache;

		$category = trim( $category );
		if ( $category === '' ) {
			return false;
		}

		if ( $parser ) {
			if ( is_bool( $wgCategoryTreeDisableCache ) && $wgCategoryTreeDisableCache === true ) {
				$parser->disableCache();
			} elseif ( is_int( $wgCategoryTreeDisableCache ) ) {
				$parser->getOutput()->updateCacheExpiry( $wgCategoryTreeDisableCache );
			}
		}

		$title = self::makeTitle( $category );

		if ( $title === false || $title === null ) {
			return false;
		}

		if ( isset( $attr['class'] ) ) {
			$attr['class'] .= ' CategoryTreeTag';
		} else {
			$attr['class'] = ' CategoryTreeTag';
		}

		$attr['data-ct-mode'] = $this->mOptions['mode'];
		$attr['data-ct-options'] = $this->getOptionsAsJsStructure();

		$html = '';
		$html .= Html::openElement( 'div', $attr );

		if ( !$allowMissing && !$title->getArticleID() ) {
			$html .= Html::openElement( 'span', array( 'class' => 'CategoryTreeNotice' ) );
			if ( $parser ) {
				$html .= $parser->recursiveTagParse( wfMessage( 'categorytree-not-found', $category )->plain() );
			} else {
				$html .= wfMessage( 'categorytree-not-found', $category )->parse();
			}
			$html .= Html::closeElement( 'span' );
			}
		else {
			if ( !$hideroot ) {
				$html .= $this->renderNode( $title, $depth, false );
			} else {
				$html .= $this->renderChildren( $title, $depth );
			}
		}

		$html .= Xml::closeElement( 'div' );
		$html .= "\n\t\t";

		return $html;
	}

	/**
	 * Returns a string with an HTML representation of the children of the given category.
	 * @param $title Title
	 * @param $depth int
	 * @return string
	 */
	function renderChildren( $title, $depth = 1 ) {
		global $wgCategoryTreeMaxChildren, $wgCategoryTreeUseCategoryTable;

		if ( $title->getNamespace() != NS_CATEGORY ) {
			// Non-categories can't have children. :)
			return '';
		}

		$dbr = wfGetDB( DB_SLAVE );

		$inverse = $this->isInverse();
		$mode = $this->getOption( 'mode' );
		$namespaces = $this->getOption( 'namespaces' );

		$tables = array( 'page', 'categorylinks' );
		$fields = array( 'page_id', 'page_namespace', 'page_title',
			'page_is_redirect', 'page_len', 'page_latest', 'cl_to',
			'cl_from' );
		$where = array();
		$joins = array();
		$options = array( 'ORDER BY' => 'cl_type, cl_sortkey', 'LIMIT' => $wgCategoryTreeMaxChildren );

		if ( $inverse ) {
			$joins['categorylinks'] = array( 'RIGHT JOIN', array( 'cl_to = page_title', 'page_namespace' => NS_CATEGORY ) );
			$where['cl_from'] = $title->getArticleID();
		} else {
			$joins['categorylinks'] = array( 'JOIN', 'cl_from = page_id' );
			$where['cl_to'] = $title->getDBkey();
			$options['USE INDEX']['categorylinks'] = 'cl_sortkey';

			# namespace filter.
			if ( $namespaces ) {
				# NOTE: we assume that the $namespaces array contains only integers! decodeNamepsaces makes it so.
				$where['page_namespace'] = $namespaces;
			} elseif ( $mode != CategoryTreeMode::ALL ) {
				if ( $mode == CategoryTreeMode::PAGES ) {
					$where['cl_type'] = array( 'page', 'subcat' );
				} else {
					$where['cl_type'] = 'subcat';
				}
			}
		}

		# fetch member count if possible
		$doCount = !$inverse && $wgCategoryTreeUseCategoryTable;

		if ( $doCount ) {
			$tables = array_merge( $tables, array( 'category' ) );
			$fields = array_merge( $fields, array( 'cat_id', 'cat_title', 'cat_subcats', 'cat_pages', 'cat_files' ) );
			$joins['category'] = array( 'LEFT JOIN', array( 'cat_title = page_title', 'page_namespace' => NS_CATEGORY ) );
		}

		$res = $dbr->select( $tables, $fields, $where, __METHOD__, $options, $joins );

		# collect categories separately from other pages
		$categories = '';
		$other = '';

		foreach ( $res as $row ) {
			# NOTE: in inverse mode, the page record may be null, because we use a right join.
			#      happens for categories with no category page (red cat links)
			if ( $inverse && $row->page_title === null ) {
				$t = Title::makeTitle( NS_CATEGORY, $row->cl_to );
			} else {
				# TODO: translation support; ideally added to Title object
				$t = Title::newFromRow( $row );
			}

			$cat = null;

			if ( $doCount && $row->page_namespace == NS_CATEGORY ) {
				$cat = Category::newFromRow( $row, $t );
			}

			$s = $this->renderNodeInfo( $t, $cat, $depth - 1 );
			$s .= "\n\t\t";

			if ( $row->page_namespace == NS_CATEGORY ) {
				$categories .= $s;
			} else {
				$other .= $s;
			}
		}

		return $categories . $other;
	}

	/**
	 * Returns a string with an HTML representation of the parents of the given category.
	 * @param $title Title
	 * @return string
	 */
	function renderParents( $title ) {
		global $wgCategoryTreeMaxChildren;

		$dbr = wfGetDB( DB_SLAVE );

		$res = $dbr->select(
			'categorylinks',
			array(
				'page_namespace' => NS_CATEGORY,
				'page_title' => 'cl_to',
			),
			array( 'cl_from' => $title->getArticleID() ),
			__METHOD__,
			array(
				'LIMIT' => $wgCategoryTreeMaxChildren,
				'ORDER BY' => 'cl_to'
			)
		);

		$special = SpecialPage::getTitleFor( 'CategoryTree' );

		$s = '';

		foreach ( $res as $row ) {
			$t = Title::newFromRow( $row );

			$label = htmlspecialchars( $t->getText() );

			$wikiLink = $special->getLocalURL( 'target=' . $t->getPartialURL() .
				'&' . $this->getOptionsAsUrlParameters() );

			if ( $s !== '' ) {
				$s .= wfMessage( 'pipe-separator' )->escaped();
			}

			$s .= Xml::openElement( 'span', array( 'class' => 'CategoryTreeItem' ) );
			$s .= Xml::openElement( 'a', array( 'class' => 'CategoryTreeLabel', 'href' => $wikiLink ) )
				. $label . Xml::closeElement( 'a' );
			$s .= Xml::closeElement( 'span' );

			$s .= "\n\t\t";
		}

		return $s;
	}

	/**
	 * Returns a string with a HTML represenation of the given page.
	 * @param $title Title
	 * @param int $children
	 * @return string
	 */
	function renderNode( $title, $children = 0 ) {
		global $wgCategoryTreeUseCategoryTable;

		if ( $wgCategoryTreeUseCategoryTable && $title->getNamespace() == NS_CATEGORY && !$this->isInverse() ) {
			$cat = Category::newFromTitle( $title );
		} else {
			$cat = null;
		}

		return $this->renderNodeInfo( $title, $cat, $children );
	}

	/**
	 * Returns a string with a HTML represenation of the given page.
	 * $info must be an associative array, containing at least a Title object under the 'title' key.
	 * @param $title Title
	 * @param $cat Category
	 * @param $children int
	 * @return string
	 */
	function renderNodeInfo( $title, $cat, $children = 0 ) {
		$mode = $this->getOption( 'mode' );

		$ns = $title->getNamespace();
		$key = $title->getDBkey();

		$hideprefix = $this->getOption( 'hideprefix' );

		if ( $hideprefix == CategoryTreeHidePrefix::ALWAYS ) {
			$hideprefix = true;
		} elseif ( $hideprefix == CategoryTreeHidePrefix::AUTO ) {
			$hideprefix = ( $mode == CategoryTreeMode::CATEGORIES );
		} elseif ( $hideprefix == CategoryTreeHidePrefix::CATEGORIES ) {
			$hideprefix = ( $ns == NS_CATEGORY );
		} else {
			$hideprefix = true;
		}

		# when showing only categories, omit namespace in label unless we explicitely defined the configuration setting
		# patch contributed by Manuel Schneider <manuel.schneider@wikimedia.ch>, Bug 8011
		if ( $hideprefix ) {
			$label = htmlspecialchars( $title->getText() );
		} else {
			$label = htmlspecialchars( $title->getPrefixedText() );
		}

		$labelClass = 'CategoryTreeLabel ' . ' CategoryTreeLabelNs' . $ns;

		if ( !$title->getArticleID() ) {
			$labelClass .= ' new';
			$wikiLink = $title->getLocalURL( 'action=edit&redlink=1' );
		} else {
			$wikiLink = $title->getLocalURL();
		}

		if ( $ns == NS_CATEGORY ) {
			$labelClass .= ' CategoryTreeLabelCategory';
		} else {
			$labelClass .= ' CategoryTreeLabelPage';
		}

		if ( ( $ns % 2 ) > 0 ) {
			$labelClass .= ' CategoryTreeLabelTalk';
		}

		$count = false;
		$s = '';

		# NOTE: things in CategoryTree.js rely on the exact order of tags!
		#      Specifically, the CategoryTreeChildren div must be the first
		#      sibling with nodeName = DIV of the grandparent of the expland link.

		$s .= Xml::openElement( 'div', array( 'class' => 'CategoryTreeSection' ) );
		$s .= Xml::openElement( 'div', array( 'class' => 'CategoryTreeItem' ) );

		$attr = array( 'class' => 'CategoryTreeBullet' );

		# Get counts, with conversion to integer so === works
		# Note: $allCount is the total number of cat members,
		# not the count of how many members are normal pages.
		$allCount = $cat ? intval( $cat->getPageCount() ) : 0;
		$subcatCount = $cat ? intval( $cat->getSubcatCount() ) : 0;
		$fileCount = $cat ? intval( $cat->getFileCount() ) : 0;

		if ( $ns == NS_CATEGORY ) {

			if ( $cat ) {
				if ( $mode == CategoryTreeMode::CATEGORIES ) {
					$count = $subcatCount;
				} elseif ( $mode == CategoryTreeMode::PAGES ) {
					$count = $allCount - $fileCount;
				} else {
					$count = $allCount;
				}
			}
			if ( $count === 0 ) {
				$bullet = wfMessage( 'categorytree-empty-bullet' )->plain() . ' ';
				$attr['class'] = 'CategoryTreeEmptyBullet';
			} else {
				$linkattr = array( );

				$linkattr[ 'class' ] = "CategoryTreeToggle";
				$linkattr['style'] = 'display: none;'; // Unhidden by JS
				$linkattr['data-ct-title'] = $key;

				$tag = 'span';
				if ( $children == 0 ) {
					$txt = wfMessage( 'categorytree-expand-bullet' )->plain();
					$linkattr[ 'title' ] = wfMessage( 'categorytree-expand' )->plain();
					$linkattr[ 'data-ct-state' ] = 'collapsed';
				} else {
					$txt = wfMessage( 'categorytree-collapse-bullet' )->plain();
					$linkattr[ 'title' ] = wfMessage( 'categorytree-collapse' )->plain();
					$linkattr[ 'data-ct-loaded' ] = true;
					$linkattr[ 'data-ct-state' ] = 'expanded';
				}

				$bullet = Xml::openElement( $tag, $linkattr ) . $txt . Xml::closeElement( $tag ) . ' ';
			}
		} else {
			$bullet = wfMessage( 'categorytree-page-bullet' )->plain();
		}
		$s .= Xml::tags( 'span', $attr, $bullet ) . ' ';

		$s .= Xml::openElement( 'a', array( 'class' => $labelClass, 'href' => $wikiLink ) )
			. $label . Xml::closeElement( 'a' );

		if ( $count !== false && $this->getOption( 'showcount' ) ) {
			$pages = $allCount - $subcatCount - $fileCount;

			global $wgContLang, $wgLang;
			$attr = array(
				'title' => wfMessage( 'categorytree-member-counts' )
					->numParams( $subcatCount, $pages , $fileCount, $allCount, $count )->text(),
				'dir' => $wgLang->getDir() # numbers and commas get messed up in a mixed dir env
			);

			$s .= $wgContLang->getDirMark() . ' ';

			# Create a list of category members with only non-zero member counts
			$memberNums = array();
			if ( $subcatCount ) {
				$memberNums[] = wfMessage( 'categorytree-num-categories' )
					->numParams( $subcatCount )->text();
			}
			if ( $pages ) {
				$memberNums[] = wfMessage( 'categorytree-num-pages' )->numParams( $pages )->text();
			}
			if ( $fileCount ) {
				$memberNums[] = wfMessage( 'categorytree-num-files' )
					->numParams( $fileCount )->text();
			}
			$memberNumsShort = $memberNums
				? $wgLang->commaList( $memberNums )
				: wfMessage( 'categorytree-num-empty' )->text();

			# Only $5 is actually used in the default message.
			# Other arguments can be used in a customized message.
			$s .= Xml::tags(
				'span',
				$attr,
				wfMessage( 'categorytree-member-num' )
					// Do not use numParams on params 1-4, as they are only used for customisation.
					->params( $subcatCount, $pages, $fileCount, $allCount, $memberNumsShort )
					->escaped()
			);
		}

		$s .= Xml::closeElement( 'div' );
		$s .= "\n\t\t";
		$s .= Xml::openElement(
			'div',
			array(
				'class' => 'CategoryTreeChildren',
				'style' => $children > 0 ? "display:block" : "display:none"
			)
		);

		if ( $ns == NS_CATEGORY && $children > 0 ) {
			$children = $this->renderChildren( $title, $children );
			if ( $children == '' ) {
				$s .= Xml::openElement( 'i', array( 'class' => 'CategoryTreeNotice' ) );
				if ( $mode == CategoryTreeMode::CATEGORIES ) {
					$s .= wfMessage( 'categorytree-no-subcategories' )->text();
				} elseif ( $mode == CategoryTreeMode::PAGES ) {
					$s .= wfMessage( 'categorytree-no-pages' )->text();
				} elseif ( $mode == CategoryTreeMode::PARENTS ) {
					$s .= wfMessage( 'categorytree-no-parent-categories' )->text();
				} else {
					$s .= wfMessage( 'categorytree-nothing-found' )->text();
				}
				$s .= Xml::closeElement( 'i' );
			} else {
				$s .= $children;
			}
		}

		$s .= Xml::closeElement( 'div' );
		$s .= Xml::closeElement( 'div' );

		$s .= "\n\t\t";

		return $s;
	}

	/**
	 * Creates a Title object from a user provided (and thus unsafe) string
	 * @param $title string
	 * @return null|Title
	 */
	static function makeTitle( $title ) {
		$title = trim( $title );

		if ( strval( $title ) === '' ) {
			return null;
		}

		# The title must be in the category namespace
		# Ignore a leading Category: if there is one
		$t = Title::newFromText( $title, NS_CATEGORY );
		if ( !$t || $t->getNamespace() != NS_CATEGORY || $t->getInterwiki() != '' ) {
			// If we were given something like "Wikipedia:Foo" or "Template:",
			// try it again but forced.
			$title = "Category:$title";
			$t = Title::newFromText( $title );
		}
		return $t;
	}

	/**
	 * Internal function to cap depth
	 * @param $mode
	 * @param $depth
	 * @return int|mixed
	 */
	static function capDepth( $mode, $depth ) {
		global $wgCategoryTreeMaxDepth;

		if ( is_numeric( $depth ) ) {
			$depth = intval( $depth );
		} else {
			return 1;
		}

		if ( is_array( $wgCategoryTreeMaxDepth ) ) {
			$max = isset( $wgCategoryTreeMaxDepth[$mode] ) ? $wgCategoryTreeMaxDepth[$mode] : 1;
		} elseif ( is_numeric( $wgCategoryTreeMaxDepth ) ) {
			$max = $wgCategoryTreeMaxDepth;
		} else {
			wfDebug( 'CategoryTree::capDepth: $wgCategoryTreeMaxDepth is invalid.' );
			$max = 1;
		}

		return min( $depth, $max );
	}
}
