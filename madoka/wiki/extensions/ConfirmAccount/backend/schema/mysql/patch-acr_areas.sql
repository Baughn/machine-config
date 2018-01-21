-- (c) Aaron Schulz, 2007

ALTER TABLE /*_*/account_requests ADD acr_areas mediumblob NOT NULL;

ALTER TABLE /*_*/account_credentials ADD acd_areas mediumblob NOT NULL;
