# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`decisions/` holds what has diverged from the frozen documents.

**Last updated:** 2026-09-07T06:47Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **One pull request per task, reviewed commit by commit.** The 400-line cap
   and the stack of branches are gone: the size limit is on the commit now —
   about 100 lines, 300 for one logical change, 1 000 split before committing —
   and a task is one branch. A push needs an adversarial local review and the
   three check tasks; a mid-task commit needs `mise run check`. Blanket staging
   refuses in `guard-bash.sh`, tested both ways — the adversarial review of the
   branch is what found the message text and `git -C`. The X13 stack finishes
   under the old rules. PR **46**.
2. **A write inside DCS never landed, and sixteen green test files could not see
   it.** In the hook state a handle's `write` and `close` return *no values at
   all* — on success and on a write to a read-only handle alike — where stock
   Lua 5.1 returns `true` from both, which is what `write_file` and
   `append_file` checked. So every write reported failure: `write_file` gave up
   before its rename and left a `.tmp` holding the right bytes, so no manifest,
   tile or config file could be written by a live run. It is ED's own C io, not
   a Lua patch in `Scripts/Hooks` — the `gui` state answers the same. ADR 0016
   checks the size that landed, and the fake reports nothing now too and can
   lose bytes. PR **44**, off `main`.
3. **The hook installs, and the window says where the run has got to.** The X13
   stack is eight branches and not seven: these two were 444 lines together.
   `window_status` is a pure function of the run and of whether a terrain is
   loaded, and the label is written only when it changed. The bootstrap reads
   the config at load, sets the log path, attaches the window and registers; an
   installed hook nobody enabled writes nothing, and the two exceptions are a
   file that will not load and an `enabled` that is a quoted boolean. Both
   verified live at the menu, window on screen. Open: **42**, **43**.
4. **X13 spiked, and five PRs open — the window is on screen.** Every unknown is
   measured on 2.9.29.27468 with the editor on Caucasus, read back by
   `DCS.makeScreenShot`. Every widget the controls need constructs from the hook
   state; an unskinned one draws nothing. A window refuses to close because the
   native side hides it and *then* fires `onClose`. `net.dostring_in("gui", ...)`
   reaches `MapWindow`: `getCurPosition()` matched the status bar to the digit,
   `getMapBounds()` answers in **kilometres**, and its Draw layer takes a
   `Polygon` whose points are *relative* to the anchor, ring closed explicitly.
   Open, bottom to top: **37** `field_problem` and `tags`, **38** the config file
   under an empty environment, **39** ADR 0014's stopped state, **40** ADR 0015's
   seam and latch, **41** the window chrome.
5. **X2a, and a change of direction: configuration moves into a window.** Most
   of the frozen config table turned out not to be a question a user can answer,
   so three ADRs came out of planning it — **0011** cuts sixteen fields to
   `enabled`, `output_dir` and a crop that is a centre and a radius, deriving or
   fixing the rest; **0012** makes a bad field one log line and its default,
   with `output_dir` alone blocking a run; **0013** makes the progress log
   append-only. A 4-PR stack.

## Next

One task. The thing to pick up immediately.

**X13 branch 8 — the controls**, on PR 43: `output_dir`, the crop as a centre and
a radius, a line per problem against the field it owns, Start writing the config
and leaving the stopped state, Stop saving the manifest. Last in the stack, so it
carries the live procedure — install, enable, watch the window appear — where
`require("Skin")` at hook-**load** time and carry 3 both get answered. Land
**44** (writes work at all) and **45** (a fresh Windows checkout builds) first,
then rebase the stack: `docs/decisions/README.md` conflicts. *Verified by:* an
agent offline; the install and the screen by a maintainer.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **X2b** — `dcs_build` from `autoupdate.cfg`, `terrain_dir` by scanning
   `Mods/terrains/*/entry.lua` for the id (ADR 0011), and the
   `terrain_fingerprint`; ADR 0007 carries the Caucasus head hashes and digest
   as its vector. The install reads 2.9.29.27468 / 20260902-093323, the build
   ADR 0007 measured.
2. **X12 / X5 / X6 / X7** — progress reporting first, so the sweeps are written
   against its `progress()` contract; then the tables, `water` and `height`, and
   roads, each an `add_job("hook", ...)`. Verified through the bridge.
3. **X8a / X8b / X8c / X9** — the mission pass, scenery, the model catalogue
   and failure handling. X ends here until `check-extract` exists.
4. **C1 / C2 / C3** — the Rust interlude X10 needs and no more: scaffold,
   `synth` on the F2 constants, `check-extract`. Closes P2 and MS0 past.
5. **X10** — the live cropped run, once C3 exists.

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
| 3 | A live run with no sweeps registered writes no output directory and reaches `done` in a few frames: `M.jobs` is three empty lists, the queue finishes at once, and `M.save` returns false with no manifest to write. Say so in X13's live testing steps or a tester will look for the extract and call the hook broken. | X13 |
