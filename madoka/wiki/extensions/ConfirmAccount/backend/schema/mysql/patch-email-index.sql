-- (c) Aaron Schulz, 2007

ALTER TABLE /*_*/account_requests ADD INDEX acr_email(acr_email(255));
