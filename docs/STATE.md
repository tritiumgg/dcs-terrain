# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-10T00:13Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **X2b: the run knows what it is extracting, and the build is the terrain's
   version.** The first prepare job reads `dcs_build` off `autoupdate.cfg`,
   finds `terrain_dir` by the shallowest id in an `entry.lua` (a directory
   without one is skipped), and records the three data files by path, size
   and payload field, each found by extension; no hash, no time, no digest,
   since terrain data ships only inside a DCS update. A step may return
   `M.REFUSED`, which stops the run with the reason on it. ADRs **0022** and
   **0023** (0021 superseded the same day). *Verified by:* an agent offline, then
   live: Starts on Caucasus and Sinai logged every size and payload the install reports, in 2 and 4 ms.
2. **X14: the window belongs in the editor, and says one thing at a time.**
   Skins read off `me_aircraft_group.dlg`, not the `_ME` family; 360 wide,
   centered; the crop block and the bar hide and the rows close up; one line
   beside one Start/Stop slot shows whatever was said last, wrapping when it
   must, with problems reworded for the screen. Validation settled two rules
   the specs left open: ADR **0019** (1 km floor, box inside the bounds
   rectangle, checked at Start, at the first terrain frame and at the pick)
   and ADR **0020** (absolute path on a drive that exists; the run still
   creates it). ED's folder dialog was measured and not built. PR **48**.
3. **X13: the window has controls, and the live pass found two bugs the offline
   tests could not.** A box for `output_dir`, a tick and three boxes for the
   crop, a line per field, Start, Stop, a bar X12 fills in, and a pick button
   that arms the next click on the map. ADR **0017**, Start re-points a run at
   the directory the boxes name. Then, on screen: buttons cut off, because rows
   go in client coordinates and a frame is a header taller; and the window
   vanishing on every screen change and map click, which looked like it being
   destroyed and was **z-order** — it was underneath all along, and `getVisible`
   says true either way. ADR **0018** raises it once at build. X14 added.
   PR **47**.
4. **One pull request per task, reviewed commit by commit.** The 400-line cap
   and the stack of branches are gone: the size limit is on the commit now —
   about 100 lines, 300 for one logical change, 1 000 split before committing —
   and a task is one branch. A push needs an adversarial local review and the
   three check tasks; a mid-task commit needs `mise run check`. Blanket staging
   refuses in `guard-bash.sh`, tested both ways — the adversarial review of the
   branch is what found the message text and `git -C`. Every earlier stack had
   landed by then, so nothing is left running on the old rules. PR **46**.
5. **A write inside DCS never landed, and sixteen green test files could not see
   it.** In the hook state a handle's `write` and `close` return *no values at
   all* — on success and on a write to a read-only handle alike — where stock
   Lua 5.1 returns `true` from both, which is what `write_file` and
   `append_file` checked. So every write reported failure: `write_file` gave up
   before its rename and left a `.tmp` holding the right bytes, so no manifest,
   tile or config file could be written by a live run. It is ED's own C io, not
   a Lua patch in `Scripts/Hooks` — the `gui` state answers the same. ADR 0016
   checks the size that landed, and the fake reports nothing now too and can
   lose bytes. PR **44**, off `main`.

## Next

One task. The thing to pick up immediately.

**X12** — progress reporting: `progress()` on a sweep, a heartbeat, and the
weighted fraction replacing `window_progress`'s equal halves. The bar and the
line it reports into are there, and the line wraps to two. Needs the ADR its
row names, for the third kind of log line. *Verified by:* an agent, offline,
over the 10 000-frame driver; the bar moving is a maintainer's eyes.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X5 / X6 / X7** — the tables, `water` and `height`, then roads, each an
   `add_job("hook", ...)` against X12's contract. X5 builds the manifest from
   `run.identity`, which X2b now fills. Verified through the bridge.
2. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
3. **C1 / C2 / C3** — the Rust interlude X10 needs: scaffold, `synth` on the F2
   constants, `check-extract`. Closes P2 and MS0 past.
4. **X10** — the 10 x 10 km Kutaisi crop that `check-extract` accepts; its
   5 km radius passes ADR 0019's checks.
5. **C4** — `pack` reads the extract: the first consumer of the X10 crop, and
   where carry 1 is discharged.

Milestone in view: **MS0 Contract** (P1, P2, F1, F2, C1, C2, C3, X1, X3).
Proof is `check-extract` accepting the Rust synthetic extract and the hook's
encoders and grid computation reproducing the F2 constants byte for byte.

First real data: **X10**, a 10 x 10 km Kutaisi extract that `check-extract`
accepts, which is where X ends for now. X11 still waits for C4 onward to read
that crop, so a field missing from the frozen format costs a crop re-run and
not a theatre. Everything from C4 on is built on macOS against the crop.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
| 1 | `synth` produces no all-fill tile at any size: the fill margin is 2 km and a tile is 12.8 km, so no tile is fill throughout. Test the absent-fill-tile read against a hand-built manifest, not against a generated extract. The all-sea case is generated, as one interior tile. | C4 |
| 2 | The `water` sweep's fill and sea skip set does not survive a restart and cannot be rebuilt from the manifest and journal. An all-fill tile is *absent* from the journal, which reads the same as not yet swept; and per-tile `min`/`max` cannot identify an all-sea tile, because they are taken over non-nodata samples only, so a tile that is part fill and part sea also reads `min = max = 2`. Rehydrate the set by re-reading the written `water` tile bytes. Getting it wrong makes `pack` read a fill cell as sea. | X6 |
| 3 | `M.retarget` has to keep pace with `M.new_run`: a field added to the run and not to the reset survives a directory change as a stale value describing the old one, which is the bug ADR 0017 exists to prevent. The key-set assertion in `statemachine.lua` fails when they drift; do not weaken it to a field list. | X5 |
