<?php

class ApiCategoryTree extends ApiBase {
	public function execute() {
		$params = $this->extractRequestParams();
		$options = array();
		if ( isset( $params['options'] ) ) {
			$options = FormatJson::decode( $params['options'] );
			if ( !is_object( $options ) ) {
				$this->dieUsage( 'Options must be valid a JSON object', 'invalidjson' );
				return;
			}
			$options = get_object_vars( $options );
		}
		$depth = isset( $options['depth'] ) ? (int)$options['depth'] : 1;

		$ct = new CategoryTree( $options );
		$depth = CategoryTree::capDepth( $ct->getOption( 'mode' ), $depth );
		$title = CategoryTree::makeTitle( $params['category'] );
		$config = $this->getConfig();
		$ctConfig = ConfigFactory::getDefaultInstance()->makeConfig( 'categorytree' );
		$html = $this->getHTML( $ct, $title, $depth, $ctConfig );

		if (
			$ctConfig->get( 'CategoryTreeHTTPCache' ) &&
			$config->get( 'SquidMaxage' ) &&
			$config->get( 'UseSquid' )
		) {
			if ( $config->get( 'UseESI' ) ) {
				$this->getRequest()->response()->header(
					'Surrogate-Control: max-age=' . $config->get( 'SquidMaxage' ) . ', content="ESI/1.0"'
				);
				$this->getMain()->setCacheMaxAge( 0 );
			} else {
				$this->getMain()->setCacheMaxAge( $config->get( 'SquidMaxage' ) );
			}
			$this->getRequest()->response()->header( 'Vary: Accept-Encoding, Cookie' ); # cache for anons only
			# TODO: purge the squid cache when a category page is invalidated
		}

		$this->getResult()->addContentValue( $this->getModuleName(), 'html', $html );
	}

	public function getConditionalRequestData( $condition ) {
		if ( $condition === 'last-modified' ) {
			$params = $this->extractRequestParams();
			$title = CategoryTree::makeTitle( $params['category'] );
			return wfGetDB( DB_SLAVE )->selectField( 'page', 'page_touched',
				array(
					'page_namespace' => NS_CATEGORY,
					'page_title' => $title->getDBkey(),
				),
				__METHOD__
			);
		}
	}

	/**
	 * Get category tree HTML for the given tree, title, depth and config
	 *
	 * @param $ct CategoryTree
	 * @param $title Title
	 * @param $depth int
	 * @param $ctConfig Config Config for CategoryTree
	 * @return string HTML
	 */
	private function getHTML( $ct, $title, $depth, $ctConfig ) {
		global $wgContLang, $wgMemc;

		$mckey = wfMemcKey(
			'ajax-categorytree',
			md5( $title->getDBkey() ),
			md5( $ct->getOptionsAsCacheKey( $depth ) ),
			$this->getLanguage()->getCode(),
			$wgContLang->getExtraHashOptions(),
			$ctConfig->get( 'RenderHashAppend' )
		);

		$touched = $this->getConditionalRequestData( 'last-modified' );
		if ( $touched ) {
			$mcvalue = $wgMemc->get( $mckey );
			if ( $mcvalue && $touched <= $mcvalue['timestamp'] ) {
				$html = $mcvalue['value'];
			}
		}

		if ( !isset( $html ) ) {
			$html = $ct->renderChildren( $title, $depth );

			$wgMemc->set(
				$mckey,
				array(
					'timestamp' => wfTimestampNow(),
					'value' => $html
				),
				86400
			);
		}
		return trim( $html );
	}

	public function getAllowedParams() {
		return array(
			'category' => array(
				ApiBase::PARAM_TYPE => 'string',
				ApiBase::PARAM_REQUIRED => true,
			),
			'options' => array(
				ApiBase::PARAM_TYPE => 'string',
			),
		);
	}

	public function isInternal() {
		return true;
	}
}
