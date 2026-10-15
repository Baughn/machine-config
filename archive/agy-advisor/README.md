# agy-advisor (archived 2026-10-15)

A second opinion from Gemini for the agents through Google's Antigravity CLI (`agy -p`), on
Baughn's Gemini subscription. Deployed on saya in 84c16bc5 and signed in once; it worked, but the
subscription tier doesn't offer the top model in agy, so it was replaced by the API-based
`machines/saya/second-opinion.nix` (Baughn, msg 1560345194488926431: "archive this code").

How it worked: `agy` (nixpkgs `antigravity-cli`) ran as its own user `agy-advisor`, which held
the sign-in (`agy-advisor-login` = `sudo -u agy-advisor agy`, URL + pasted code). A
socket-activated `agy-advisor@` unit (/run/agy-advisor/socket, group agy-advisor-clients) took
one JSON request per connection and ran `agy --print --mode plan --dangerously-skip-permissions
--disable-slash-commands` in an allowlisted repository, read through tmpfiles ACLs. agy brings
its own `search_web` and `read_url_content` tools; it also has a shell tool, and the sign-in
carries the `cloud-platform` scope.

To revive it: move both files back to machines/saya/, import agy-advisor.nix there, and sign in.
