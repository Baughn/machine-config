{ config, lib, ... }:

# saya's agent reaches the board over wg0 with a bearer token (agent-board.nix). Until Baughn
# has created secrets/agent-board-saya-token.age (agenix -e; a random string, also used by
# machines/saya/agents.nix), the TCP listener stays off.
#
# saya-client (machines/saya/agent-client.nix) gets its own token the same way, from
# secrets/agent-board-saya-client-token.age.
let
  tokenFile = ../../secrets/agent-board-saya-token.age;
  clientTokenFile = ../../secrets/agent-board-saya-client-token.age;
  hasClient = builtins.pathExists clientTokenFile;
in
lib.mkIf (builtins.pathExists tokenFile) {
  age.secrets = {
    agent-board-saya-token.file = tokenFile;
  } // lib.optionalAttrs hasClient {
    agent-board-saya-client-token.file = clientTokenFile;
  };
  me.agentBoard = {
    httpAddress = "10.171.0.1:8740";
    tokens = {
      saya = config.age.secrets.agent-board-saya-token.path;
    } // lib.optionalAttrs hasClient {
      saya-client = config.age.secrets.agent-board-saya-client-token.path;
    };
  };
}
