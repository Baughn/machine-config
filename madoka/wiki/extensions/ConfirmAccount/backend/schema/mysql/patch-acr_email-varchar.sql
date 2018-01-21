-- Make this field easier to index
ALTER TABLE /*_*/account_requests MODIFY /*i*/acr_email VARCHAR(255) binary NOT NULL;