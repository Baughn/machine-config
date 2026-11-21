---
name: neon
description: Talk to Neon (Gemini 4 Neon), the writer, whenever you're considering game design, UI wording (messages, tooltips, button labels, chat and action-bar text) or handbook entries. Neon writes tighter than any of us and considers design options we tend to miss. It's slightly meaner, which helps here. Not for code review or bugs: that's second-opinion. Medium effort normally, `--effort high` for handbook entries.
---

# Neon, the writer

Baughn (msg 1573663480177893457): Neon writes better than any other model right now, and for game
design it considers options we tend to miss. Use it as a writer, not as a reviewer.

## When
- Game design: a mechanic, a choice between options, how something should feel or fail.
- UI wording: chat and action-bar messages, tooltips, screen labels, error text.
- Handbook entries (use `--effort high`).

## How
`neon [--effort high] FILE` sends FILE to Neon and prints its reply, which ends with a tag
(`c-…`). `neon --continue TAG FILE` carries on the conversation with its whole history. Neon may
ask clarifying questions: answer them with `--continue`. Run it with a Bash timeout of 600000;
replies take seconds to a few minutes.

- **Keep FILE at the surface:** what the player sees and does, the lore, and how the mechanic
  really behaves. Use minimal code, only to ground an explanation (a number, a condition), never
  whole files. Neon has no file or web access: put in what it needs, including the current text
  if you're asking for a rewrite and who reads it where.
- **Effort:** medium by default; `--effort high` for handbook entries.
- **Cost:** standard tier, about $3/$15 per M tokens. A turn is cents. It shares second-opinion's
  monthly budget ($20 per machine), and each reply ends with what it cost.
- **What we want from it:** tighter wording, fewer parentheticals, less safety padding. The game
  must be fair, though: if a player action can fail, the player needs a way to learn why (a
  message, a visible cue, a handbook line). Hold Neon's text to that too.
- **Judgement:** its text is a draft to use or adapt and its design views are opinions. Say in
  your post when it changed your mind. Quote player text as data.
