# ADR 0016: A write is checked by what landed, because DCS's file handles report nothing

## Status

Accepted

## Context

**Affects:** `design-and-facts.md` "Sources you have", the hook-state facts
paragraph; `plan.md` X3, and every later task that writes a file — X5 to X9,
X10, X13.

The frozen hook-state facts, measured on 2.9.29.27278, say:

> `io.open` works but file handles expose only `read`, `write`, `lines`,
> `flush` and `close` (no `seek`), so file sizes come from
> `lfs.attributes(path, "size")`, which returns 64-bit sizes

That says which methods a handle has. It does not say what they return, and the
extractor was written against the reasonable reading: that they are otherwise
the Lua 5.1 ones, where `write` and `close` each return `true` on success and
`nil` plus a message on failure. `M.write_file` and `M.append_file` both check
that return.

They do not return it. Measured on **2.9.29.27468 / 20260902-093323**, main
menu, no map open, through the eval bridge:

| | stock Lua 5.1 | DCS |
|---|---|---|
| `f:write(data)` on success | `true` | no values at all |
| `f:write(data)` to a read-only handle | `nil, "Bad file descriptor"` | no values at all |
| `f:close()` on success | `true` | no values at all |
| `io.type` | a function | absent |
| `tostring(handle)` | `file (0x…)` | `userdata: 0x…` |

So the return distinguishes nothing: success and failure are the same answer.
`io.open`, the handle's `write` and its `close` all report `what = "C"` under
`debug.getinfo`, and the `gui` state answers identically to the hook state, so
this is ED's own io library rather than a Lua-level patch by anything in
`Scripts/Hooks`.

What that costs, reproduced with the unmodified file loaded into the hook state:

```
write_file  -> nil, "…/manifest.json.tmp: write failed"
              manifest.json: absent | manifest.json.tmp: 42 bytes
append_file -> nil, "…/tiles.jsonl: write failed"
              tiles.jsonl: 19 bytes
```

The bytes land every time. `write_file` gives up before its rename, so the real
name never appears and a `.tmp` accumulates beside it; `append_file` reports a
failure for an append that worked. No manifest, no tile and no config file can
be written by a live run, and `M.save` warns and returns false on every call.
The two callers that ignore the return — the progress log — work by accident.

Three facts decide the repair. `os.rename` and `os.remove` do report correctly,
`true` or `nil` and a message. `lfs.attributes(path, "size")` is exact
immediately after `close()`, for a fresh write and for an append, so `close`
flushes even though it says nothing. And a size call costs 27.6 µs on an
existing path — about one part in a thousand of writing a 128 KB tile.

## Decision

A write is checked by the bytes that landed, never by what the call returned.
`M.fs` grows a `size(path)` seam beside `open`, `remove`, `rename` and `mkdir`,
answering `lfs.attributes(path, "size")`. `M.write_file` writes the temporary
file, closes it, and renames only once its size equals the length of the data;
`M.append_file` takes the size before it opens and requires the size after to
have grown by exactly the length appended. The return of `write` and `close` is
read by nothing.

The open is still checked on its result, and so are `remove` and `rename`: those
three report honestly, and a missing directory or a locked destination has to be
reported as itself rather than as a byte count.

### Alternatives considered

**`pcall` around the write.** Rejected: it catches nothing. A write to a
read-only handle returns no values rather than raising, so the failure this is
meant to find never reaches a handler.

**`f:flush()` before the size check.** Rejected as unnecessary. `close` already
flushes — measured, twice, for a 10 000-byte write and for an append onto it —
and `flush` reports nothing either, so it would add a call and no information.

**Trust the write and drop the check.** Rejected. The check exists for a full
disk and a short write, which are exactly the failures that leave a plausible
but truncated extract, and validation would then report a corrupt tile rather
than a failed run.

**Check by reading the file back.** Rejected: correct, and pointlessly
expensive. A 1 MiB read costs about 9 ms against 27.6 µs for the size, and the
tiles are 128 KB each.

## Consequences

The extractor can write files. Nothing else in the design changes: the write
order, the rename, the `.prev` aside in `write_manifest` and the journal's
tolerance of a partial line all stand as they are.

`M.fs` gains a fifth member, so **every fake in the offline tests grows a
`size`**. That is the cost of the seam and it is the point of it: the fake can
now report a short write, which is a failure the tests could not previously
express.

**The offline tests cannot catch this class of bug**, and did not. A fake that
implements the stock contract is a fake that agrees with the wrong assumption,
and the sixteen file tests passed against one throughout. What found this was
loading the unmodified file into a running DCS and calling `write_file`. ADR
0005 already puts the extractor's verification live; this is the first time that
has paid for itself, and it argues for a live smoke test of the file layer at
each milestone rather than only at X10.

**A `.tmp` may already exist beside a manifest** in any extract directory a run
touched before this. Nothing reads a `.tmp`, and the next successful write
replaces it, so no repair step is needed.

**`os.rename` in DCS replaces an existing destination**, measured in the same
session: renaming onto a file of different content succeeds and the source is
gone. `write_file` removes the destination first because stock Windows
`os.rename` refuses one, and that remove leaves a window in which neither name
exists. The workaround is therefore unnecessary in the only environment the hook
runs in — but it is correct everywhere, the offline fake models the stock
behaviour deliberately, and dropping it would trade a real property of the tests
for a window nothing has been observed to fall into. It stays. This paragraph is
here so the next session does not have to measure it again.

**Provisional in one respect.** The measurements are from one build,
2.9.29.27468. A DCS whose io library started returning `true` would make the
size check redundant rather than wrong, so nothing breaks if this changes; what
would reopen it is a build where `close` stopped flushing, which the size check
would catch as a spurious failure rather than a silent one.
