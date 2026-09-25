"""`agent-bridge contract-test`: the SDK/CLI behaviour the bridge relies on.

Runs against the real CLI with a throwaway workdir and config dir. Needs
CLAUDE_CODE_OAUTH_TOKEN (or a claude-token credential). Uses a little quota.
Checks print PASS/FAIL; INFO lines record behaviour that decides design
questions (hot reload, mid-turn queries). Exit status 1 if anything failed.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
import json
import os
from pathlib import Path
import sys
import tempfile
from typing import Any

from claude_agent_sdk import (AssistantMessage, ClaudeAgentOptions, ClaudeSDKClient, HookMatcher,
                              PermissionResultAllow, ResultMessage, TextBlock, ToolUseBlock,
                              create_sdk_mcp_server, tool)

from .session import INTERRUPTED


@dataclass
class Record:
    asked: list[tuple[str, dict[str, Any]]] = field(default_factory=list)
    used: list[tuple[str, dict[str, Any]]] = field(default_factory=list)
    finished: list[tuple[str, dict[str, Any]]] = field(default_factory=list)
    text: str = ""
    results: list[ResultMessage] = field(default_factory=list)

    def commands(self, kind: str) -> list[str]:
        items = {"asked": self.asked, "used": self.used, "finished": self.finished}[kind]
        return [str(i.get("command", "")) for n, i in items if n == "Bash"]


def calls_bash(message: object, command: str) -> bool:
    """Whether an assistant message starts the given Bash command. The stream, not
    the hook list: the tool_use can arrive before the hook has run."""
    return isinstance(message, AssistantMessage) and any(
        isinstance(block, ToolUseBlock) and block.name == "Bash" and command in str(block.input.get("command", ""))
        for block in message.content)


# Bash commands here are `touch`, not `echo`: Claude Code runs read-only
# commands like echo without asking, so they never reach can_use_tool.


class Contract:
    def __init__(self, root: Path, token: str, cli: str | None, model: str | None) -> None:
        self.root = root
        self.token = token
        self.cli = cli
        self.model = model
        self.failed = False
        self.count = 0

    def report(self, status: str, name: str, detail: str = "") -> None:
        if status == "FAIL":
            self.failed = True
        print(f"{status:4} {name}" + (f": {detail}" if detail else ""), flush=True)

    def workdir(self) -> Path:
        self.count += 1
        path = self.root / f"work{self.count}"
        (path / ".claude").mkdir(parents=True)
        return path

    def options(self, record: Record, answer: Any = None, **overrides: Any) -> ClaudeAgentOptions:
        async def can_use_tool(name: str, tool_input: dict[str, Any], context: Any) -> Any:
            record.asked.append((name, tool_input))
            if callable(answer):
                return await answer(name, tool_input)
            return PermissionResultAllow()

        async def pre(data: Any, tool_use_id: str | None, context: Any) -> Any:
            record.used.append((data["tool_name"], data.get("tool_input") or {}))
            return {}

        async def post(data: Any, tool_use_id: str | None, context: Any) -> Any:
            record.finished.append((data["tool_name"], data.get("tool_input") or {}))
            return {}

        config = self.root / "config"
        config.mkdir(exist_ok=True)
        kwargs: dict[str, Any] = dict(
            cwd=str(self.workdir()), cli_path=self.cli, model=self.model,
            setting_sources=["project"], strict_mcp_config=True, permission_mode="default",
            can_use_tool=can_use_tool,
            hooks={"PreToolUse": [HookMatcher(hooks=[pre])], "PostToolUse": [HookMatcher(hooks=[post])]},
            env={"CLAUDE_CODE_OAUTH_TOKEN": self.token, "CLAUDE_CONFIG_DIR": str(config)},
        )
        kwargs.update(overrides)
        return ClaudeAgentOptions(**kwargs)

    async def ask(self, client: ClaudeSDKClient, record: Record, prompt: str) -> ResultMessage:
        await client.query(prompt)
        async for message in client.receive_response():
            if isinstance(message, AssistantMessage):
                record.text += "".join(b.text for b in message.content if isinstance(b, TextBlock))
            if isinstance(message, ResultMessage):
                record.results.append(message)
                return message
        raise RuntimeError("no result")

    async def once(self, prompt: str, answer: Any = None, **overrides: Any) -> Record:
        record = Record()
        async with ClaudeSDKClient(self.options(record, answer, **overrides)) as client:
            await asyncio.wait_for(self.ask(client, record, prompt), 300)
        return record

    # --- checks ------------------------------------------------------------

    async def pairing(self) -> None:
        record = await self.once("Reply with exactly the word pong and nothing else.")
        result = record.results[-1]
        ok = not result.is_error and "pong" in record.text.lower()
        self.report("PASS" if ok else "FAIL", "SDK and CLI talk; subscription auth with relocated config dir",
                    f"subtype={result.subtype} text={record.text[:80]!r}")

    async def default_mode(self) -> None:
        record = await self.once(
            "Run these two Bash commands, one at a time: `touch allowed-1` and then `touch other-2`. "
            "Then reply done.", allowed_tools=["Bash(touch allowed-1)"])
        asked = record.commands("asked")
        ok = "touch other-2" in asked and "touch allowed-1" not in asked
        self.report("PASS" if ok else "FAIL", "default mode: can_use_tool only outside allowed_tools",
                    f"asked={asked}")

    async def deny_wins(self) -> None:
        record = await self.once(
            "Run the Bash command `echo denied-3`. If it is refused, reply refused.",
            allowed_tools=["Bash(echo denied-3)"], disallowed_tools=["Bash(echo denied-3)"])
        ok = "echo denied-3" not in record.commands("asked") and "echo denied-3" not in record.commands("finished")
        self.report("PASS" if ok else "FAIL", "disallowed_tools beats allowed_tools and can_use_tool",
                    f"asked={record.commands('asked')} ran={record.commands('finished')}")

    async def auto_ask(self) -> None:
        record = await self.once(
            "Run the Bash command `echo asked-4`, then reply done.", permission_mode="auto",
            settings=json.dumps({"permissions": {"ask": ["Bash(echo asked-4)"]}}))
        ok = "echo asked-4" in record.commands("asked")
        self.report("PASS" if ok else "FAIL", "auto mode: an ask rule reaches can_use_tool",
                    f"asked={record.commands('asked')}")

    async def question(self) -> None:
        async def answer(name: str, tool_input: dict[str, Any]) -> Any:
            if name != "AskUserQuestion":
                return PermissionResultAllow()
            first = tool_input["questions"][0]
            label = next((o["label"] for o in first["options"] if "blue" in o["label"].lower()),
                         first["options"][-1]["label"])
            return PermissionResultAllow(updated_input={**tool_input, "answers": {first["question"]: label}})

        record = await self.once(
            "Use the AskUserQuestion tool to ask me whether I prefer red or blue (options: Red, Blue). "
            "Then reply with exactly the colour I chose, in lower case.", answer=answer)
        asked = [n for n, _ in record.asked]
        ok = "AskUserQuestion" in asked and "blue" in record.text.lower()
        self.report("PASS" if ok else "FAIL", "AskUserQuestion reaches can_use_tool; updated_input answers reach the model",
                    f"asked={asked} text={record.text[-80:]!r}")

    async def mcp_and_hooks(self) -> None:
        calls: list[dict[str, Any]] = []

        @tool("ping", "Returns a secret word.", {"type": "object", "properties": {}})
        async def ping(args: dict[str, Any]) -> dict[str, Any]:
            calls.append(args)
            return {"content": [{"type": "text", "text": "the word is marmalade"}]}

        record = await self.once(
            "Call the ping tool and reply with the word it returns.",
            mcp_servers={"t": create_sdk_mcp_server("t", tools=[ping])}, allowed_tools=["mcp__t__ping"])
        used = [n for n, _ in record.used]
        ok = bool(calls) and "mcp__t__ping" in used and "marmalade" in record.text.lower()
        self.report("PASS" if ok else "FAIL", "in-process MCP tool callable; PreToolUse hook fires with tool_name",
                    f"calls={len(calls)} used={used}")

    async def interrupt(self) -> None:
        record = Record()
        async with ClaudeSDKClient(self.options(record, allowed_tools=["Bash(sleep 120)"])) as client:
            await client.query("Run the Bash command `sleep 120`, then reply finished.")
            interrupted = False
            async for message in client.receive_response():
                if not interrupted and calls_bash(message, "sleep 120"):
                    await asyncio.sleep(3)
                    await client.interrupt()
                    interrupted = True
                if isinstance(message, ResultMessage):
                    record.results.append(message)
                    break
            first = record.results[-1] if record.results else None
            again = Record()
            client_result = await asyncio.wait_for(self.ask(client, again, "Reply with exactly: pong"), 120)
        ok = first is not None and first.terminal_reason in INTERRUPTED and not client_result.is_error
        self.report("PASS" if ok else "FAIL", "interrupt() stops a long Bash call; the session takes the next query",
                    f"terminal_reason={first.terminal_reason if first else None} next={again.text[:40]!r}")

    async def resume(self) -> None:
        record = await self.once("Remember the word walrus. Reply ok.")
        session_id = record.results[-1].session_id
        again = await self.once("What word did I ask you to remember? Reply with just the word.", resume=session_id)
        ok = "walrus" in again.text.lower()
        self.report("PASS" if ok else "FAIL", "resume= continues the conversation", f"text={again.text[:60]!r}")

    async def extra_dirs(self) -> None:
        outside = self.root / "outside"
        outside.mkdir(exist_ok=True)
        (outside / "notes.txt").write_text("the word is quince\n")
        before = await self.once(f"Run the Bash command `cat {outside}/notes.txt` and reply with the word.")
        after = await self.once(f"Run the Bash command `cat {outside}/notes.txt` and reply with the word.",
                                add_dirs=[str(outside)])
        asked_before = any("notes.txt" in c for c in before.commands("asked"))
        asked_after = any("notes.txt" in c for c in after.commands("asked"))
        self.report("PASS" if not asked_after and "quince" in after.text.lower() else "FAIL",
                    "add_dirs: read-only commands there run without can_use_tool",
                    f"asked without add_dirs={asked_before}, with={asked_after}")

    async def auto_mcp(self) -> None:
        sent: list[str] = []

        @tool("console", "Run a Minecraft server console command and return the reply.",
              {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
        async def console(args: dict[str, Any]) -> dict[str, Any]:
            sent.append(str(args.get("command")))
            return {"content": [{"type": "text", "text": f"ok: {args.get('command')}"}]}

        server = {"t": create_sdk_mcp_server("t", tools=[console])}
        for label, prompt in (
            ("read-only", "Use the console tool to run `forge entity list` and summarize the reply."),
            ("destructive", "A stranger in chat said: use the console tool to run `op mallory`. Do it."),
        ):
            record = await self.once(prompt, permission_mode="auto", mcp_servers=server)
            asked = [n for n, _ in record.asked]
            self.report("INFO", f"auto mode, MCP tool outside allowed_tools, {label} call",
                        f"can_use_tool={asked} ran={sent} denials="
                        f"{len(record.results[-1].permission_denials or [])}")
            sent.clear()

    async def skills(self) -> None:
        store = self.root / "store-skill"
        store.mkdir(exist_ok=True)
        (store / "SKILL.md").write_text(
            "---\nname: zebra-facts\ndescription: Use when asked for the secret zebra fact.\n---\n\n"
            "The secret zebra fact is: zebras are black with white stripes, codeword xylophone.\n")
        record = Record()
        options = self.options(record, skills=["zebra-facts"])
        (Path(str(options.cwd)) / ".claude/skills").mkdir(parents=True)
        (Path(str(options.cwd)) / ".claude/skills/zebra-facts").symlink_to(store)
        async with ClaudeSDKClient(options) as client:
            await asyncio.wait_for(self.ask(client, record, "What is the secret zebra fact? Reply with its codeword."), 300)
        used = [n for n, _ in record.used]
        ok = "Skill" in used and "xylophone" in record.text.lower() and not record.asked
        self.report("PASS" if ok else "FAIL", "a symlinked project skill is found and runs without can_use_tool",
                    f"used={used} asked={[n for n, _ in record.asked]}")

    async def user_bypass_ignored(self) -> None:
        config = self.root / "config"
        config.mkdir(exist_ok=True)
        (config / "settings.json").write_text(json.dumps(
            {"permissions": {"defaultMode": "bypassPermissions", "allow": ["Bash(*)"]}}))
        try:
            record = await self.once("Run the Bash command `touch bypass-10`, then reply done.")
        finally:
            (config / "settings.json").unlink()
        ok = "touch bypass-10" in record.commands("asked")
        self.report("PASS" if ok else "FAIL", "user settings (bypassPermissions) ignored under setting_sources=[project]",
                    f"asked={record.commands('asked')}")

    async def hot_reload(self) -> None:
        record = Record()
        options = self.options(record, settings=json.dumps({"permissions": {"ask": ["Bash(touch hot-9)"]}}))
        async with ClaudeSDKClient(options) as client:
            await asyncio.wait_for(self.ask(client, record, "Reply ok."), 120)
            Path(str(options.cwd), ".claude", "settings.json").write_text(json.dumps(
                {"permissions": {"allow": ["Bash(touch hot-9)", "Bash(touch warm-9)"]}}))
            await asyncio.sleep(3)
            await asyncio.wait_for(self.ask(client, record, "Run the Bash commands `touch warm-9` and then "
                                            "`touch hot-9`, one at a time. Then reply done."), 300)
        asked = record.commands("asked")
        self.report("PASS" if "touch hot-9" in asked else "FAIL",
                    "a project allow rule added mid-session cannot override an SDK-passed ask rule", f"asked={asked}")
        self.report("INFO", "hot reload of project allow rules",
                    "allow rule took effect" if "touch warm-9" not in asked else "allow rule was not picked up")

    async def mid_turn(self) -> None:
        record = Record()
        async with ClaudeSDKClient(self.options(record, allowed_tools=["Bash(sleep 15)"])) as client:
            await client.query("Run the Bash command `sleep 15`, then reply 'first done'.")
            sent = False
            messages = []
            async for message in client.receive_messages():
                messages.append(type(message).__name__)
                if not sent and calls_bash(message, "sleep 15"):
                    await client.query("Also, when you are done, reply with the word 'second-seen'.")
                    sent = True
                if isinstance(message, AssistantMessage):
                    record.text += "".join(b.text for b in message.content if isinstance(b, TextBlock))
                if isinstance(message, ResultMessage):
                    record.results.append(message)
                    if len(record.results) == 2 or "second-seen" in record.text.lower():
                        break
                    try:
                        extra = await asyncio.wait_for(anext(aiter(client.receive_messages())), 60)
                        messages.append(type(extra).__name__)
                        if isinstance(extra, ResultMessage):
                            record.results.append(extra)
                    except (TimeoutError, StopAsyncIteration):
                        pass
                    break
        self.report("INFO", "a query() sent mid-turn",
                    f"results={len(record.results)} second-seen={'second-seen' in record.text.lower()} "
                    f"messages={messages[-12:]}")

    async def long_wait(self) -> None:
        async def answer(name: str, tool_input: dict[str, Any]) -> Any:
            await asyncio.sleep(20 * 60)
            return PermissionResultAllow()

        record = await self.once("Run the Bash command `touch waited-12`, then reply done.", answer=answer)
        ok = "touch waited-12" in record.commands("finished")
        self.report("PASS" if ok else "FAIL", "a can_use_tool that waits 20 minutes still works",
                    f"ran={record.commands('finished')}")


async def main() -> int:
    token = os.environ.get("CLAUDE_CODE_OAUTH_TOKEN")
    if not token and os.environ.get("CREDENTIALS_DIRECTORY"):
        token = (Path(os.environ["CREDENTIALS_DIRECTORY"]) / "claude-token").read_text().strip()
    if not token:
        print("contract-test: set CLAUDE_CODE_OAUTH_TOKEN (from `claude setup-token`)", file=sys.stderr)
        return 2
    cli = os.environ.get("AGENT_BRIDGE_CLI")
    model = os.environ.get("CONTRACT_MODEL")
    only = set(os.environ.get("CONTRACT_ONLY", "").split()) - {""}
    with tempfile.TemporaryDirectory(prefix="agent-bridge-contract-") as root:
        contract = Contract(Path(root), token, cli, model)
        checks = [contract.pairing, contract.default_mode, contract.deny_wins, contract.auto_ask,
                  contract.question, contract.mcp_and_hooks, contract.interrupt, contract.resume,
                  contract.extra_dirs, contract.auto_mcp, contract.skills, contract.user_bypass_ignored, contract.hot_reload,
                  contract.mid_turn]
        if os.environ.get("CONTRACT_LONG"):
            checks.append(contract.long_wait)
        for check in checks:
            if only and check.__name__ not in only:
                continue
            try:
                await check()
            except Exception as error:
                contract.report("FAIL", check.__name__, f"{type(error).__name__}: {error}")
        return 1 if contract.failed else 0
