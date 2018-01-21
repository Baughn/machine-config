<?php
namespace GoogleLogin;

/**
 * Same as HTMLSubmitField, the only difference is, that the style module to
 * style the Google button (according Googles guidelines) is added.
 */
class HTMLGoogleLoginButtonField extends \HTMLSubmitField {
	public function getInputHTML( $value ) {
		$this->addGoogleButtonStyleModule();
		return parent::getInputHTML( $value );
	}

	public function getInputOOUI( $value ) {
		$this->addGoogleButtonStyleModule( 'ooui' );
		return parent::getInputOOUI( $value );
	}

	/**
	 * Adds the required style module to the OutputPage object for styling of the Login
	 * with Google button.
	 *
	 * @param string $target Defines which style module should be added (vform, ooui)
	 */
	private function addGoogleButtonStyleModule( $target = "vform" ) {
		if ( $this->mParent instanceof HTMLForm ) {
			$out = $this->mParent->getOutput();
		} else {
			$out = \RequestContext::getMain()->getOutput();
		}
		if ( $target === 'vform' ) {
			$out->addModuleStyles( 'ext.GoogleLogin.userlogincreate.style' );
		} else {
			$out->addModuleStyles( 'ext.GoogleLogin.userlogincreate.ooui.style' );
		}
	}
}
