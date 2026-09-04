## Summary

<!--
A paragraph a reader can stop after: what the change does and why, ending
with what it is reviewed against — the task id in docs/plan.md, the decision
record, or for a stacked branch the claim this branch alone makes.
-->

## Details

<!--
What Summary left out: the choice behind the change and the alternatives it
passed over, what the diff leaves alone and why, and anything the reviewer
needs to know before reading it. Not a file list.
-->

## README

<!--
What this change alters in what a user downloads, installs, configures or
runs, and the README paragraph that now says so. Name any "not usable yet",
"not final" or "planned" note the change takes out. Write "none" when nothing
a user sees changed.
-->

## Testing

<!--
How anyone tests this change, not a record of who has. Start from a clean
checkout. Write "none" under a heading with nothing in it, rather than
removing the heading.

The last branch in a stack carries the steps. An intermediate branch writes
"Covered by <the last branch>" here and names it, because one procedure
copied onto five branches drifts into five procedures. That does not weaken
the rule that every branch stands on its own: its own tests still pass and CI
still gates it. What moves is the procedure a human follows, which is only
meaningful against the stack as merged.

Every step is one of two kinds, each an imperative sentence:

  - an action: "Run `mise run check`." "Load Kutaisi and take the slot."
  - a check: "Verify the journal holds one line per tile and no duplicate."

Place a Verify step wherever the tester needs to know the steps so far worked
before going on. Several actions, a Verify, more actions, another Verify is
the expected shape.

Without DCS: builds, unit tests, the offline Lua tests, the document tools,
the synthetic theatre. Windows steps in PowerShell, macOS and Linux steps in
bash. Where the two are the same command, give it once under "All platforms".

With DCS: this needs a Windows machine with DCS installed. Build the
artifacts, copy the hook into Saved Games (say the exact path and edit), then
what to do in DCS, with a Verify after each thing the tester should see.
Never write into the DCS install.

Not covered: every part of the task's completion condition these steps do not
reach, and where that gap is recorded — docs/STATE.md, an issue, or the plan.
-->

### Without DCS

#### All platforms

1.

#### Windows (PowerShell)

1.

#### macOS and Linux (bash)

1.

### With DCS (Windows)

1.

### Not covered

## Results

<!--
Optional. What running the steps produced: log excerpts, measurements,
screenshots, a run that failed and why. Delete the section if empty.
-->
