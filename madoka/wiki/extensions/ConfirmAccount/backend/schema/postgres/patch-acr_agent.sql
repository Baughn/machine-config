BEGIN;

ALTER TABLE account_requests
	ADD acr_xff TEXT,
	ADD acr_agent TEXT;

ALTER TABLE account_credentials
	ADD acd_xff TEXT,
	ADD acd_agent TEXT;

COMMIT;
