# The lab agent's weekly consolidation turn (Baughn, msgs 1573294827234402326 and
# 1573320112629485619, choices in the AskUserQuestion answer that followed): find and fix
# duplicated helpers, logic, tests and UX after the fact, and fact-check the wiki.
# Saturday night, when Baughn usually has spare quota. Unattended, like free research.
{
  calendar = "Sat *-*-* 04:00:00";
  thread = "Consolidation";
  instruction = ''
    Weekly consolidation, from Baughn: find what has grown twice and make it one again, and check
    that the wiki still says what is true. Scope: Electrical Age, Stasis, Watershed, lab-tools,
    and the wiki pages you own. Keep notes in consolidation/<date>/ (the date is in the note).
    The bridge opens a Discord thread for this turn; your posts go there by default.

    1. Find (cheap, noisy on purpose): fan out subagents on Sonnet or Haiku, one per area of a
       repo (networks and routing, UI/HUD, items and tools, GameTest helpers, build scripts...).
       Each lists duplicates: two helpers doing the same thing, divergent copies of one piece
       of logic, overlapping tests, two UXs for the same action. Start from the cases we already
       know (pipe vs cable routing, the identical 48 kW plant tests, the build-*.sh copies) to see
       whether the finders catch them.
    2. Verify: check each finding yourself or with a stronger subagent, or a 2-of-3 vote. Is it real,
       which copy should survive, how big is the fix? Drop what doesn't hold up.
    3. Fix or ask:
       - Pure dedup (no change in behaviour or UX: same tests pass, nothing a player would notice)
         you may land through the merge queue without asking Baughn first, in small commits.
       - Anything that changes design or UX goes to Baughn as a short list (keep A / keep B /
         merge, with your pick). Don't build it yet.
    4. Record: name the surviving canonical helpers and invariants in the repo's REVIEW.md,
       so the pre-push review points at them next time.
    5. Wiki: one cheap subagent per page you own lists its concrete claims (numbers, names, "live
       since X", procedures) and checks each against the code, config or cited thread. Fix what's
       wrong. For pages LabAssistant owns, put what you find in a board post asking it.
    Finish with a short summary post: what you merged, what waits on Baughn, which pages changed.
    Rules:
    - Baughn is asleep: approval requests and questions are refused automatically. If something
      you wanted to land needs one, leave it on a branch and say so in the summary.
    - Don't ask other agents to act on the live servers, or to do anything that needs an approval.
    - This may be a fresh session: your handoff and notes are your context.
  '';
}
