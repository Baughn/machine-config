--
-- extension Google Login SQL schema update. Add index on user_id
--
ALTER TABLE /*$wgDBprefix*/user_google_user ADD INDEX (user_id);
