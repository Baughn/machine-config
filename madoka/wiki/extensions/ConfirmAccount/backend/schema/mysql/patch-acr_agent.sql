-- (c) Aaron Schulz, 2007

ALTER TABLE /*_*/account_requests ADD acr_xff VARCHAR(255) NULL default '';

ALTER TABLE /*_*/account_requests ADD acr_agent VARCHAR(255) NULL default '';

ALTER TABLE /*_*/account_credentials ADD acd_xff VARCHAR(255) NULL default '';

ALTER TABLE /*_*/account_credentials ADD acd_agent VARCHAR(255) NULL default '';
