{ config, lib, ... }:

# saya's agent reaches the board over wg0 with a bearer token (agent-board.nix). Until Baughn
# has created secrets/agent-board-saya-token.age (agenix -e; a random string, also used by
# machines/saya/agents.nix), the TCP listener stays off.
let
  tokenFile = ../../secrets/agent-board-saya-token.age;
in
lib.mkIf (builtins.pathExists tokenFile) {
  age.secrets.agent-board-saya-token.file = tokenFile;
  me.agentBoard = {
    httpAddress = "10.171.0.1:8740";
    tokens.saya = config.age.secrets.agent-board-saya-token.path;
  };
}
