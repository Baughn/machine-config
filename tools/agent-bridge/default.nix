{ pkgs, ... }:

pkgs.python3Packages.buildPythonApplication {
  pname = "agent-bridge";
  version = "0.1.0";
  pyproject = true;
  src = ./.;

  build-system = [ pkgs.python3Packages.setuptools ];
  dependencies = with pkgs.python3Packages; [ discordpy claude-agent-sdk ];

  postPatch = ''
    substituteInPlace agent_bridge/shellfmt.py \
      --replace-fail 'SHFMT = "shfmt"' 'SHFMT = "${pkgs.shfmt}/bin/shfmt"'
    substituteInPlace agent_bridge/gitdiff.py \
      --replace-fail 'GIT = "git"' 'GIT = "${pkgs.git}/bin/git"'
  '';

  nativeCheckInputs = (with pkgs.python3Packages; [ pytestCheckHook pytest-asyncio hypothesis mypy ]) ++ [ pkgs.git ];
  preCheck = ''
    mypy
  '';
  pythonImportsCheck = [ "agent_bridge" ];

  meta.mainProgram = "agent-bridge";
}
