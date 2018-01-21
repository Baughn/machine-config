-- (c) Aaron Schulz, 2007

ALTER TABLE /*_*/account_requests ADD acr_filename VARCHAR(255) NULL default '';

ALTER TABLE /*_*/account_requests ADD acr_storage_key VARCHAR(64) NULL default '';

ALTER TABLE /*_*/account_requests ADD acr_held binary(14) default '';

ALTER TABLE /*_*/account_requests ADD acr_comment VARCHAR(255) NULL default '';
