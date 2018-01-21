--
-- extension Google Login SQL schema
--
CREATE TABLE /*$wgDBprefix*/user_google_user (
  user_googleid DECIMAL(25,0) unsigned NOT NULL PRIMARY KEY,
  user_id int(10) unsigned NOT NULL,
  KEY(user_id)
) /*$wgDBTableOptions*/;
