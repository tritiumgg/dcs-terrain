# ADR-0005: The extractor is verified live; offline Lua tests are minimal

- **Status:** Accepted
- **Date:** 2026-09-03
- **Affects:** `extractor-hook.md` "Testing"; `plan.md` tasks P1, P2, X1, X3,
  X4, X5, X6, X7, X8a, X8b, X8c, X9 and milestone MS0.

## Context

The extractor hook's tests were specified against a vendored stub harness.
`extractor-hook.md` "Testing" says:

> Run under `extractor/test/StubHarness.lua`, a copy of
> `dcs-scripting/scripts/StubHarness.lua` (1 197 lines; it stubs
> `net.dostring_in` and passes `require` through, so the fake `terrain`
> module installs through `package.loaded`) vendored into the repository
> so CI does not depend on a skill directory

and requires of the result:

> The harness run must produce an extract directory
> that `dcsterrain check-extract` accepts and that is sample-for-sample
> identical to the Rust generator's output for the same parameters.

That comparison is milestone MS0's proof, and every X done test in
`plan.md` is phrased as "Harness ...".

Two things are now true that were not when those documents were written.
The `dcs-scripting` skill is no longer a dependency of this project, so the
file the harness was to be copied from is not available; `CLAUDE.md` carries
that prohibition and the replacements for it. And the `dcs-api-bridge` MCP
tools can drive a running DCS directly, so the sweeps can be checked against
a real theatre instead of against stubs of one.

The two are not symmetric. A stub is a faithful substitute for the parts of
the hook that never touch DCS — encoder arithmetic, tile addressing, the
journal, the frame budget. It is a poor substitute for the sweeps, whose
failures come from real theatre tables, real fill behaviour and real object
ids, none of which a stub reproduces. Conversely a running DCS cannot be made
to deliver 10 000 frames on demand, nor to change terrain mid-run, so the
frame-budget and failure-injection tests have no live form.

## Decision

The extractor keeps offline Lua tests only where DCS cannot be driven on
demand: the encoders, grid computation from each bounds source, the journal
and resume, the frame-budget state machine, and injected failures. Those tests
supply their own fakes, sized to what the task under test calls, and no stub
harness is vendored into the repository. The sweeps that read terrain are
verified through the `dcs-api-bridge` MCP against a running theatre on the
Windows machine. The sample-for-sample comparison between a harness extract
and the Rust synthetic extract is not built; the extract format contract is
proven at X10 instead, where `check-extract` accepts a real cropped extract
and cells are re-read through the bridge.

## Consequences

The sweeps are checked against the data that actually breaks them, and the
repository carries no vendored stub file it did not write.

MS0 loses its cross-language proof. The format frozen at F1 is therefore
unverified until X10, so a missing or mistyped field is found later, on the
Windows machine, and costs a re-run of the crop. F1's done test carries more
weight than it did, which is why it now names the consumers its sign-off must
walk.

Extractor development moves to the Windows machine, because the sweeps cannot
be developed anywhere else. That machine is otherwise reserved for gaming.

CI no longer runs a full extractor sweep. P2's Lua clause covers the offline
unit tests only.

The done tests of P1, P2, X1, X3, X4, X5, X6, X7, X8a, X8b, X8c and X9 change,
as does MS0's proof. `extractor-hook.md` "Testing" now reads false in its
first sentence and in its sample-for-sample requirement.

## Alternatives considered

**Write a stub harness in this repository.** It would keep MS0's proof and an
offline CI sweep. It loses because the stub surface is large, this project
would own it, and it is weakest exactly where the sweeps are most likely to
break — a stub that reproduces the fill triple and the airdrome table
correctly is a second implementation of the thing under test.

**Leave the shape open until X1 is written.** It loses because nine done tests
and one milestone proof would stay unstated while F1 and P1 are being written,
and those are the next two tasks.
