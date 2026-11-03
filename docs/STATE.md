# State

Where the work is. Updated at the end of every working session, before handing
back. This file holds progress; `plan.md` holds the task graph and
`design.md` and `queries.md` hold the design.

**Last updated:** 2026-11-03T06:49Z

## Done

Newest first. Max 5 entries — drop the oldest when adding a sixth.

1. **M3: setup and the skill through MCP.** The skill calls the tools when
   present, one call per batch line, and a `score_site` over the ring for the
   FARP exposure; the guide sets up Claude Code and Claude Desktop.
   *Verified by:* an agent: on Caucasus of 2.9.31.29434 every FARP call
   through `mcp` equals `query`, each ring share its 36 `visible` calls, and a
   stand-in given only the skill wrote calls with those answers. The user's
   run, an assistant with no shell, is pending.
2. **M2: the long-lived MCP server.** A cancelled call stops its work at
   the next look; `find_sites` and `exposure` report progress to the
   caller's token; a GeoJSON, then an answer, over 64 KiB goes to a file
   in the temp directory; a terrain whose files change reopens.
   *Verified by:* an agent, through `mise run check` on `Synth`: cancels in
   a 22 s search's reads and its sightlines and in a sweep each stop within
   1 s; opt-in, 100 calls on Caucasus of 2.9.31.29434 peak at 63.0 to 63.5
   MiB, as the same lines through `query` do at 63.6.
3. **M1: `dcsterrain mcp`.** An MCP server on stdio through `rmcp` 3.8.0,
   one tool per operation, its input the request schema cut from `schema`,
   each call answered by the opener a batch uses; a refusal is a tool error
   holding the issue list. A large answer is held whole as one text until M2.
   *Verified by:* an agent, through `mise run check`: an rmcp client over the
   binary's stdio matches every tool schema to `schema --op` and every answer
   to `query`'s, byte for byte but for the time taken, refusals included.
4. **R2: the guide.** `docs/guide.md` walks a siting question on Caucasus from
   install to Mission Editor position, combines questions into a hidden FARP and
   its convoy route, draws GeoJSON, and covers each failure; questions go in on
   stdin, which Windows PowerShell 5.1 passes intact. README rewritten. Fixes:
   airdrome display names accepted; `not_ready` says not built. *Verified by:*
   an agent reran every command in 5.1 and 7 on 2.9.31.29434, and a stand-in
   newcomer subagent answered an unseen question from the guide alone; the live
   newcomer test was not run: the user reviewed the guide and judged it adequate,
   the tool being mainly for assistants and software.
5. **R1: release builds.** `.github/workflows/release.yml`: a `v*` tag builds
   Windows, Linux and both macOS archives on their own runners, the C runtime
   static on Windows and Linux, and releases them with `SHA256SUMS`. *Verified
   by:* CI: on each platform a job holding only the archive answered
   `query --terrain synth describe`; v0.1.0 released, its Windows archive
   answering the README's example.

## Next

One task. The thing to pick up immediately.

**C1** — the models a name and a box cannot judge, from screenshots.
*Verified by:* live: an agent drives the capture with the user at DCS.

## Then

Max 5 entries, in dependency order. Task ids from `plan.md`.

1. **H1** — one parser for every place a flag takes. *Agent, opt-in.*
2. **H2** — one subcommand per operation. *Agent.*
3. **H4** — GeoJSON on `sample`, `nearest` and `scenery_in`. *Agent.*
4. **H3** — readable output for the main operations. *Agent.*
5. **H5** — the README and the guide for people. *User: no DCS.*

M and H run in parallel beside C1, which is live (ADR 0011). Milestone in
view: **MS3 Assistant and release**, which C1 closes; then **MS4 Interfaces**.

## Carries

Things a later task must not lose. Max 10 — `CLAUDE.md` has the rules.

| # | Carry | Discharged by |
|---|---|---|
