# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-02T18:01Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **R2: the guide.** `docs/guide.md` walks a siting question on Caucasus from
   install to Mission Editor position, combines questions into a hidden FARP and
   its convoy route, draws GeoJSON, and covers each failure; questions go in on
   stdin, which Windows PowerShell 5.1 passes intact. README rewritten. Fixes:
   airdrome display names accepted; `not_ready` says not built. *Verified by:*
   an agent reran every command in 5.1 and 7 on 2.9.31.29434, and a stand-in
   newcomer subagent answered an unseen question from the guide alone; the live
   newcomer test was not run: the user reviewed the guide and judged it adequate,
   the tool being mainly for assistants and software.
2. **R1: release builds.** `.github/workflows/release.yml`: a `v*` tag builds
   Windows, Linux and both macOS archives on their own runners, the C runtime
   static on Windows and Linux, and releases them with `SHA256SUMS`. *Verified
   by:* CI: on each platform a job holding only the archive answered
   `query --terrain synth describe`; no tag pushed yet.
3. **Towns by cluster.** A place is any building names do not rule out, sized by
   floor at about 100 m² a person, industrial buildings only linking; no
   per-terrain overrides. Census 0.63; Afghanistan and the Marianas have towns
   and cities (`data/places.md`). *Verified by:* an agent, through
   `mise run check`, and opt-in on the 8 terrains of 2.9.31.29434.
4. **N13: passages in the road graph.** Routes search the road file's own table
   and connectors, every edge is decoded, and the sortie keeps refused routes
   (ADR 0009). *Verified by:* an agent, opt-in on Caucasus of 2.9.31.29434 against
   V1's replies: 22 of 22 refused, 198 of 198 answered, largest +0.24 %; worst peak
   60.0 MiB on 8 terrains.
5. **C1 on Caucasus: the screenshot method.** Camera and screenshot calls
   proven, 241 models shot and judged, a rule that a name saying no building
   makes `misc`, 17 models moved, method in `plan.md`
   (`data/scenery-review.md`). *Verified by:* live: an agent drove the capture
   and read every frame with the user at DCS, on 2.9.31.29434.

## Next

One task. The thing to pick up immediately.

**C1** — the models a name and a box cannot judge, from screenshots.
*Verified by:* live: an agent drives the capture with the user at DCS.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

None: C1 closes MS3.

Milestone in view: **MS3 Assistant and release**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
