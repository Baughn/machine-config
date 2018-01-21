<?php
class ApiGoogleLoginInfo extends ApiBase {
	public function execute() {
		$apiResult = $this->getResult();
		$params = $this->extractRequestParams();
		$glConfig = ConfigFactory::getDefaultInstance()->makeConfig( 'googlelogin' );
		$user = $this->getUser();

		if ( !isset( $params['googleid'] ) ) {
			$this->dieUsage( 'Invalid Google ID', 'googleidinvalid' );
		}

		// only user with the managegooglelogin right can use this Api
		if ( !$user->isAllowed( 'managegooglelogin' ) ) {
			$this->dieUsage(
				'Insufficient permissions. You need the managegooglelogin permission to use this API module',
				'insufficientpermissions'
			);
		}

		$googleUser = \GoogleLogin\GoogleUser::newFromGoogleId( $params['googleid'] );
		if ( !$googleUser->isDataLoaded() ) {
			$this->dieUsage( 'Google user not found or false api key.', 'unknownuser' );
		}
		$result = [];
		if ( $googleUser->getData( 'displayName' ) ) {
			$result[$this->msg( 'googlelogin-googleuser' )->text()] = $googleUser->getData( 'displayName' );
		}
		if ( $googleUser->getData( 'image' ) ) {
			$result['profileimage'] = $googleUser->getData( 'image' )['url'];
		}
		if ( $googleUser->getData( 'isPlusUser' ) ) {
			$result[$this->msg( 'googlelogin-manage-isplusser' )->text()] =
				$googleUser->getData( 'isPlusUser' );
		}
		if ( is_array( $googleUser->getData( 'organizations' ) ) ) {
			$org = $googleUser->getData( 'organizations' )[0];
			if ( $org['primary'] ) {
				$result[$this->msg( 'googlelogin-manage-orgname' )->text()] = $org['name'];
			}
			if ( $org['title'] ) {
				$result[$this->msg( 'googlelogin-manage-orgtitle' )->text()] = $org['title'];
			}
			if ( $org['startDate'] ) {
				$result[$this->msg( 'googlelogin-manage-orgsince' )->text()] = $org['startDate'];
			}
		}
		// build result array
		$r = [
			'success' => true,
			'result' => $result
		];
		// add result to API output
		$apiResult->addValue( null, $this->getModuleName(), $r );
	}

	public function getAllowedParams() {
		return [
			'googleid' => [
				ApiBase::PARAM_TYPE => 'string',
			],
		];
	}
}
