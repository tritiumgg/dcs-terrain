# ADR 0020: The output directory is an absolute path on a drive that exists

## Status

Accepted

## Context

**Affects:** `extractor-hook.md` "Config table", whose `output_dir` is a
string `created if absent`; ADR 0012's validation; `plan.md` X10, X13, X14.

The frozen config table says what `output_dir` is for and that the run
creates it, and nothing about what a usable value looks like. The checker
that ADR 0011 gave the window refused an empty string and a control
character, and nothing else: `asdf` passed, and so did `C:/ex?tract`.

Both fail later and worse. A relative path is made under DCS's working
directory, which `lfs.currentdir()` reports as the install root — and the
install is never written into. A path with a character Windows refuses fails
in `mkdir` at prepare, after a Start the window accepted. A drive letter that
is not mounted fails the same way, since a drive cannot be made.

The run's own `mkdir_p` makes every missing component below the root, so the
directory itself need not exist. The window was given one line to say what
is wrong before a run is spent, which is where these belong.

## Decision

`output_dir` is an absolute path — a drive letter followed by a separator, or
a UNC root with a server and a share — with either separator, and contains
none of `< > " | ? *` and no colon beyond the drive letter's. The checker
refuses anything else, in the log's words and the window's, before the value
is normalized. Separately, at the first frame and at Start, the path's root
has to be a directory the disk reports: `output_dir's drive Q:/ does not
exist`. The directory below the root is still created by the run, as the
frozen line says.

### Alternatives considered

**Require the directory to exist.** Built and withdrawn the same day: it
turns "created if absent" false and adds a step on first use, against a
mistake the shape and drive checks already catch.

**A folder dialog.** ED's `FileDialog.selectFolder` is reachable from the
hook and modal, but its OK button enters a highlighted folder rather than
choosing it, and fixing that means patching the editor's own handler for the
duration of the call. The text box stays.

## Consequences

A mistyped path is refused at the press, with the reason, rather than at
prepare with a `mkdir` error in the log. `created if absent` stays true.

**The rules are Windows's.** The hook runs only where DCS does, so that is
not a loss, but a path that Windows would take and these rules refuse — a
device path, a drive-relative `C:file` — is refused too. None has been seen.

**Provisional in one respect.** A UNC share's root is checked with
`lfs.attributes`, which has not been measured against a share that needs
credentials; a share that answers slowly answers at the press.
