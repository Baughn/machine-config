<?php

namespace OOUI;

class WikimediaUITheme extends Theme {

	/* Methods */

	public function getElementClasses( Element $element ) {
		$variants = [
			'warning' => false,
			'invert' => false,
			'progressive' => false,
			'constructive' => false,
			'destructive' => false
		];

		// Parent method
		$classes = parent::getElementClasses( $element );

		if ( $element->supports( [ 'hasFlag' ] ) ) {
			$isFramed = $element->supports( [ 'isFramed' ] ) && $element->isFramed();
			$isActive = $element->supports( [ 'isActive' ] ) && $element->isActive();
			if ( $isFramed && ( $isActive || $element->isDisabled() || $element->hasFlag( 'primary' ) ) ) {
				// Button with a dark background, use white icon
				$variants['invert'] = true;
			} elseif ( !$isFramed && $element->isDisabled() ) {
				// Frameless disabled button, always use black icon regardless of flags
				$variants['invert'] = false;
			} elseif ( !$element->isDisabled() ) {
				// Any other kind of button, use the right colored icon if available
				$variants['progressive'] = $element->hasFlag( 'progressive' );
				$variants['constructive'] = $element->hasFlag( 'constructive' );
				$variants['destructive'] = $element->hasFlag( 'destructive' );
				$variants['warning'] = $element->hasFlag( 'warning' );
			}
		}

		foreach ( $variants as $variant => $toggle ) {
			$classes[$toggle ? 'on' : 'off'][] = 'oo-ui-image-' . $variant;
		}

		return $classes;
	}
}
