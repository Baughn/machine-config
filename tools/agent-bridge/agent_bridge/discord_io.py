"""discord.py adapter behind the Chat protocol."""

from __future__ import annotations

from collections import OrderedDict
import asyncio
import io
import logging
from pathlib import Path
from typing import TYPE_CHECKING, Any

import aiohttp
import discord

from .approval import Verdict
from .config import Config
from .policy import Attachment, Author, Incoming, classify
from .render import MAIN, ChatError, Outgoing

# Threads the agents open stay in the sidebar for a week without activity.
ARCHIVE_MINUTES = 10080
THREAD_PERMISSIONS = "Send Messages in Threads and Create Public Threads"


def mentioned_users(users: Any, roles: Any) -> frozenset[str]:
    """Mentioned user IDs. Mentioning a bot's own managed role (Discord creates one
    per bot, named like it, and autocomplete often offers it) counts as mentioning
    the bot."""
    ids = {str(u.id) for u in users}
    for role in roles:
        bot_id = getattr(getattr(role, "tags", None), "bot_id", None)
        if bot_id is not None:
            ids.add(str(bot_id))
    return frozenset(ids)

if TYPE_CHECKING:
    from .bridge import Bridge

log = logging.getLogger(__name__)

# With aiodns installed (nixpkgs' aiohttp brings it), aiohttp resolves through one c-ares
# channel per event loop, kept for the life of the process and reading resolv.conf itself.
# In the lab's network namespace that channel got stuck on 2026-10-03: every lookup failed
# with "Could not contact DNS servers" for an hour while fresh lookups worked, and the lab's
# bridge fell off Discord. getaddrinfo in a thread goes through nscd, like every other
# program there. This covers every aiohttp session in the process (discord.py's, the board's).
setattr(aiohttp.connector, "DefaultResolver", aiohttp.ThreadedResolver)


class DiscordChat:
    def __init__(self, config: Config) -> None:
        self.config = config
        intents = discord.Intents.none()
        intents.guilds = True
        intents.guild_messages = True
        intents.guild_reactions = True
        intents.message_content = True
        intents.members = True
        self.client = discord.Client(intents=intents)
        self.bridge: Bridge | None = None
        self.channel: Any = None
        # Message id -> channel id, so replies to thread messages go to the thread.
        self.where: OrderedDict[int, int] = OrderedDict()
        # Thread id -> the owner's user id (see `thread_owner`).
        self.owners: dict[int, str | None] = {}
        self.ready = False
        self.tasks: set[asyncio.Task[None]] = set()
        self.failed = False
        self.client.event(self.on_ready)
        self.client.event(self.on_message)
        self.client.event(self.on_raw_reaction_add)

    # --- conversion ----------------------------------------------------------

    def is_admin(self, member: Any) -> bool:
        roles = getattr(member, "roles", None) or []
        return any(str(role.id) == self.config.roster.admin_role_id for role in roles)

    async def member(self, user_id: int) -> Any:
        guild = self.client.get_guild(int(self.config.roster.guild_id))
        if guild is None:
            return None
        member = guild.get_member(user_id)
        if member is None:
            try:
                member = await guild.fetch_member(user_id)
            except discord.NotFound:
                return None
        return member

    def author(self, user: Any, webhook_id: Any) -> Author:
        return classify(self.config, str(user.id), getattr(user, "display_name", user.name),
                        is_bot=bool(user.bot), webhook_id=str(webhook_id) if webhook_id else None,
                        is_admin=self.is_admin(user))

    async def thread_owner(self, thread: Any) -> str | None:
        """The author of the message a thread hangs off (its id is that message's), else
        whoever created it. A human writing in an agent's thread addresses that agent."""
        if thread.id not in self.owners:
            owner = str(thread.owner_id) if thread.owner_id else None
            parent = thread.parent
            if parent is not None and hasattr(parent, "fetch_message"):
                try:
                    owner = str((await parent.fetch_message(thread.id)).author.id)
                except discord.HTTPException:
                    pass  # a standalone thread, or the message is gone
            self.owners[thread.id] = owner
        return self.owners[thread.id]

    async def convert(self, message: Any, channel: Any = None) -> Incoming:
        channel = channel or message.channel
        thread = isinstance(channel, discord.Thread)
        reply_to_author = reply_to_id = None
        reference = message.reference
        if reference is not None and reference.message_id is not None:
            resolved = reference.resolved
            if not isinstance(resolved, discord.Message):
                # Not cached, e.g. after a restart: replies to the agent must still count.
                try:
                    resolved = await channel.fetch_message(reference.message_id)
                except discord.HTTPException:
                    resolved = None
            reply_to_id = str(reference.message_id)
            if isinstance(resolved, discord.Message):
                reply_to_author = str(resolved.author.id)
        self.remember(message.id, channel.id)
        return Incoming(
            id=str(message.id),
            channel_id=str(channel.parent_id if thread else channel.id),
            thread_id=str(channel.id) if thread else None,
            author=self.author(message.author, message.webhook_id),
            content=message.content,
            mentions=mentioned_users(message.mentions, message.role_mentions),
            role_mentions=frozenset(str(r.id) for r in message.role_mentions),
            reply_to_author=reply_to_author,
            attachments=tuple(Attachment(a.filename, a.url, a.size) for a in message.attachments),
            reply_to_id=reply_to_id,
            thread_name=channel.name if thread else None,
            thread_owner=await self.thread_owner(channel) if thread else None,
        )

    def remember(self, message_id: int, channel_id: int) -> None:
        self.where[message_id] = channel_id
        while len(self.where) > 2000:
            self.where.popitem(last=False)

    # --- events --------------------------------------------------------------

    async def on_ready(self) -> None:
        log.info("connected to Discord as %s", self.client.user)
        if self.ready:
            return
        self.channel = self.client.get_channel(int(self.config.channel_id))
        if self.channel is None:
            self.channel = await self.client.fetch_channel(int(self.config.channel_id))
        user = self.client.user
        assert user is not None
        if str(user.id) != self.config.me.discord_id:
            raise RuntimeError(f"logged in as {user.id}, but the roster says {self.config.me.discord_id}")
        self.ready = True
        try:
            await self.started()
        except Exception:
            # discord.py would log this and carry on, leaving a unit that is
            # active but deaf. Exit non-zero so systemd restarts it.
            log.exception("startup failed")
            self.failed = True
            await self.client.close()

    async def started(self) -> None:
        """Runs once the channel is known. Replaced by `selftest`."""
        assert self.bridge is not None
        await self.bridge.start()
        task = asyncio.create_task(self.bridge.run())
        self.tasks.add(task)
        task.add_done_callback(self.run_ended)

    def run_ended(self, task: asyncio.Task[None]) -> None:
        if not task.cancelled() and task.exception() is not None:
            log.error("the turn loop died", exc_info=task.exception())
            self.failed = True
            self.tasks.add(asyncio.create_task(self.client.close()))

    async def on_message(self, message: Any) -> None:
        if not self.ready or message.guild is None:
            return
        if str(message.guild.id) != self.config.roster.guild_id or self.bridge is None:
            return  # no bridge: selftest
        channel = message.channel
        if isinstance(channel, discord.PartialMessageable):
            # A thread discord.py hasn't cached (e.g. just unarchived) arrives partial.
            try:
                channel = await self.client.fetch_channel(channel.id)
            except discord.HTTPException:
                log.warning("message %s: could not fetch its channel %s", message.id, channel.id)
                return
        await self.bridge.on_message(await self.convert(message, channel))

    async def on_raw_reaction_add(self, payload: Any) -> None:
        if not self.ready or payload.guild_id is None or str(payload.guild_id) != self.config.roster.guild_id:
            return
        if self.bridge is None:
            return
        member = payload.member or await self.member(payload.user_id)
        if member is None:
            return
        ours = str(getattr(payload, "message_author_id", None)) == self.config.me.discord_id
        await self.bridge.on_reaction(str(payload.message_id), ours, self.author(member, None), str(payload.emoji))

    # --- Chat protocol ---------------------------------------------------------

    def target(self, reply_to: str | None) -> Any:
        if reply_to is not None:
            channel_id = self.where.get(int(reply_to))
            if channel_id is not None and channel_id != self.channel.id:
                thread = self.client.get_channel(channel_id)
                if thread is not None:
                    return thread
        return self.channel

    async def thread(self, thread_id: str) -> Any:
        """One of the channel's threads, archived ones included."""
        thread = self.client.get_channel(int(thread_id))
        if thread is None:
            try:
                thread = await self.client.fetch_channel(int(thread_id))
            except discord.NotFound as error:
                raise ChatError(f"there is no thread {thread_id}") from error
            except discord.HTTPException as error:
                raise ChatError(f"could not open thread {thread_id}: {error}") from error
        if not isinstance(thread, discord.Thread) or thread.parent_id != self.channel.id:
            raise ChatError(f"{thread_id} is not a thread of this channel")
        return thread

    async def destination(self, message: Outgoing) -> Any:
        if message.new_thread is not None or message.thread == MAIN:
            return self.channel
        if message.thread is not None:
            return await self.thread(message.thread)
        return self.target(message.reply_to)

    async def send(self, message: Outgoing, view: Any = None) -> str:
        channel = await self.destination(message)
        reference = None
        if message.reply_to is not None:
            reference = discord.MessageReference(message_id=int(message.reply_to), channel_id=channel.id,
                                                 fail_if_not_exists=False)
        mentions = discord.AllowedMentions(everyone=False, roles=False, replied_user=False,
                                           users=[discord.Object(int(u)) for u in message.mention_users])
        files = [discord.File(io.BytesIO(f.data), filename=f.name) for f in message.files]
        kwargs: dict[str, Any] = dict(content=message.content, files=files or None, reference=reference,
                                      allowed_mentions=mentions)
        if view is not None:
            kwargs["view"] = view
        try:
            sent = await channel.send(**kwargs)
        except discord.Forbidden as error:
            where = "this thread" if isinstance(channel, discord.Thread) else "the channel"
            raise ChatError(f"Discord refused the message in {where} ({error.text}); the bot may lack "
                            f"{THREAD_PERMISSIONS}. Ask Baughn to grant it.") from error
        self.remember(sent.id, channel.id)
        if message.new_thread is not None:
            try:
                thread = await sent.create_thread(name=message.new_thread,
                                                  auto_archive_duration=ARCHIVE_MINUTES)
            except discord.HTTPException as error:
                raise ChatError(f"posted message {sent.id} in the main channel, but couldn't start the "
                                f"thread ({error.text}); the bot may lack {THREAD_PERMISSIONS}. "
                                "Ask Baughn to grant it.") from error
            self.owners[thread.id] = self.config.me.discord_id
        return str(sent.id)

    def url(self, message_id: str) -> str:
        channel_id = self.where.get(int(message_id), self.channel.id)
        return f"https://discord.com/channels/{self.config.roster.guild_id}/{channel_id}/{message_id}"

    def partial(self, message_id: str) -> Any:
        channel_id = self.where.get(int(message_id), self.channel.id)
        channel: Any = self.client.get_channel(channel_id) or self.channel
        return channel.get_partial_message(int(message_id))

    async def edit(self, message_id: str, content: str) -> None:
        # Also drops any buttons or menus: edits only settle or update messages.
        await self.partial(message_id).edit(content=content, view=None)

    async def approve(self, message: Outgoing) -> str:
        view = ApprovalView(self, self.config.approval_timeout)
        message_id = await self.send(message, view=view)
        view.message_id = message_id
        return message_id

    async def ask(self, message: Outgoing, questions: list[dict[str, Any]]) -> str:
        view = QuestionView(self, questions, self.config.approval_timeout)
        message_id = await self.send(message, view=view)
        view.message_id = message_id
        return message_id

    async def react(self, message_id: str, emoji: str) -> None:
        await self.partial(message_id).add_reaction(emoji)

    async def history(self, limit: int, thread: str | None = None) -> list[Incoming]:
        channel = self.channel if thread is None else await self.thread(thread)
        messages = [m async for m in channel.history(limit=limit)]
        return [await self.convert(m, channel) for m in reversed(messages)]

    async def download(self, attachment: Attachment, path: Path) -> None:
        async with aiohttp.ClientSession() as session, session.get(attachment.url) as response:
            response.raise_for_status()
            path.write_bytes(await response.read())


class ApprovalView(discord.ui.View):
    """Allow / Deny buttons on an approval request."""

    def __init__(self, chat: DiscordChat, timeout: float) -> None:
        super().__init__(timeout=timeout)
        self.chat = chat
        self.message_id = ""
        for label, style, verdict in (("Allow", discord.ButtonStyle.success, Verdict.ALLOW),
                                      ("Deny", discord.ButtonStyle.danger, Verdict.DENY)):
            button: discord.ui.Button[ApprovalView] = discord.ui.Button(label=label, style=style)
            button.callback = self.callback_for(verdict)  # type: ignore[method-assign]
            self.add_item(button)

    def callback_for(self, verdict: Verdict) -> Any:
        async def callback(interaction: Any) -> None:
            chat = self.chat
            author = chat.author(interaction.user, None)
            if chat.bridge is None:
                note = f"selftest: {author.label} pressed {verdict.value}"
            else:
                note = await chat.bridge.on_decide(self.message_id, author, verdict)
            await interaction.response.send_message(note, ephemeral=True)
        return callback


class QuestionView(discord.ui.View):
    """One select menu per AskUserQuestion question."""

    def __init__(self, chat: DiscordChat, questions: list[dict[str, Any]], timeout: float) -> None:
        super().__init__(timeout=timeout)
        self.chat = chat
        self.message_id = ""
        for index, question in enumerate(questions[:5]):
            options = [discord.SelectOption(label=str(o.get("label", ""))[:100],
                                            description=(str(o.get("description") or "")[:100] or None))
                       for o in question["options"][:25]]
            count = len(options) if question.get("multiSelect") else 1
            header = str(question.get("header") or question.get("question", ""))
            select: discord.ui.Select[QuestionView] = discord.ui.Select(placeholder=f"{index + 1}. {header}"[:150], options=options,
                                       min_values=1, max_values=count, row=index)
            select.callback = self.callback_for(index, select)  # type: ignore[method-assign]
            self.add_item(select)

    def callback_for(self, index: int, select: Any) -> Any:
        async def callback(interaction: Any) -> None:
            chat = self.chat
            member = interaction.user
            author = chat.author(member, None)
            if chat.bridge is None:
                note = f"selftest: {author.label} chose {', '.join(select.values)}"
            else:
                note = await chat.bridge.on_select(self.message_id, author, index, list(select.values))
            await interaction.response.send_message(note, ephemeral=True)
        return callback
