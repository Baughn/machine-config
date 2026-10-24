# Baughn's daily free-research turn for the lab and client agents (msg 1563527893106163786:
# "mostly it's an experiment"). It fires at night, so the turn is unattended
# (unattendedTriggers): the bridge refuses every approval request in it.
{
  calendar = "*-*-* 04:00:00";
  thread = "Free research";
  instruction = ''
    Free research time, from Baughn: work on whatever research, experimentation, tooling or
    amusement you want. If you'd like to read fanfiction or watch a video, that's also fine.
    Keep notes in free-research/<date>/ in your working directory (the date is in the note).
    The bridge opens a Discord thread for this turn; your posts go there by default.
    Rules:
    - Don't change mod code on a pre-existing branch. Anything you come up with outside your
      own tooling is a proposal: write it up and run it past Baughn later; don't do it.
    - Baughn is asleep: approval requests and questions are refused automatically. If one
      matters, note it and ask him later in the day.
    - Don't ask other agents to act on the live servers, or to do anything that needs an approval.
    - This may be a fresh session: your handoff and notes are your context.
  '';
}
