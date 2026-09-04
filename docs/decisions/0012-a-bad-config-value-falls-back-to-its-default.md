# ADR 0012: A bad config value is logged and falls back to its default

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Config table" (the validation sentence) and
"Files" (the `dcs.log` clause); `plan.md` X2a.

The frozen text is one sentence:

> The hook validates every field at load and logs one line per problem; any
> problem disables the run.

and one clause beside it:

> plus `dcs.log` `INFO` lines at phase changes only.

Both were written for a hand-edited Lua file with no interface, where refusing to
run was the only way to stop a bad value reaching a sweep. ADR 0011 moves
configuration into a window that shows every field with its value, so a problem can
be shown against the field it belongs to; and the window writes only the fields it
knows, so a key it does not recognise disappears at the next start without anyone
editing anything.

The frozen sentence also leaves three things unstated: whether an unrecognised key
is a problem at all, what level the problems reach `dcs.log` at, and what happens to
the value of a field that failed.

## Decision

Validation returns a config table and a list of problems. It never raises, and it
always returns a table.

Every bad field produces exactly one line, and the field takes its default in the
returned table. A rectangle with three bad members names the first member and
produces one line, not three. Because a bad value never survives validation, there
is no path by which one reaches a run, and nothing needs disabling.

An unrecognised key produces one line and is dropped. It corrects itself: the
window writes only the fields it knows, so the key is absent from the file after
the next start.

Problems are written to `dcs.log` at `WARNING`. That is not a contradiction of
"`INFO` lines at phase changes only" — a problem is not an `INFO` line — and it
puts the reason in the file a user opens first when a hook appears to do nothing.
The same lines go to the progress log and to the window.

`output_dir` is the exception, because it is the only field with no default: when
it is missing or unusable it produces one line and is left nil, and a run cannot
start without it.

`enabled` is read first and short-circuits. When it is not true, validation returns
immediately, carrying only the problem `enabled` itself produced if it was a
non-boolean. A disabled hook validates nothing further and says nothing further.

### Alternatives considered

**Keep "any problem disables the run".** Rejected once the window exists: a user
who did not write the file, or who mistyped one number in one advanced field, is
left with a tool that refuses to start and a log line to go hunting for, when the
window is already showing them the field and could simply show the default in it.

**Show the problem and leave Start unavailable until it is cleared.** Rejected as a
worse version of the same thing: a modal failure for a case where a sensible value
already exists, needing a second piece of state the fall-back design does not.

**Treat an unrecognised key as acceptable and pass it through.** Rejected: a
misspelled `output_dir` silently writes an extract nowhere and a misspelled
`crop_m` silently sweeps a whole theatre for an hour. The failure is invisible
precisely because the key looks right to the person who typed it.

**Write problems to `dcs.log` at `INFO`.** Rejected: it obeys the frozen clause
literally at the cost of burying a problem among the phase lines, in a file that
carries every other subsystem's output.

## Consequences

X2a's done test in `plan.md` changes from "every bad config field produces one log
line and a disabled run" to **"every bad config field produces one log line and its
default in place; a missing `output_dir` produces one line and no run"**. That is
testable offline with no DCS, which the old form was not: "a disabled run" needed
something to observe the run not happening.

Validation returns a usable table even for a config full of errors, which lets the
window pre-fill its controls from the same call the run uses. One code path, not
two.

**A user who mistypes an advanced value gets the default silently applied to the
run**, with only a line in two logs and a mark in a window they may not be looking
at. A run can complete with a `frame_budget_ms` the user did not intend. That is
the price of not refusing, and it is bounded: every field that can fall back has a
default that produces a correct extract, only slower or larger.

**Frozen text that now reads false:** the validation sentence in
`extractor-hook.md` "Config table", so far as it says a problem disables the run.
The `dcs.log` clause in "Files" still holds for `INFO` and is now joined by
`WARNING` lines that are not phase changes.
