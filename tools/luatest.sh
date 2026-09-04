#!/bin/sh
# Run the offline extractor tests under Lua 5.1.
#
# Every .lua file directly under extractor/test/ is a test. They run from the
# repository root and set their own package.path, so this changes directory
# there first and passes the path as given.
#
# 5.1 is not a preference: it is the version the DCS hook state runs, and these
# tests exist to catch what 5.1 does differently. On Windows the interpreter is
# the one `mise run lua51` builds into .tools/bin; elsewhere it is the 5.1.5
# mise installs. Both answer to `lua`.
#
# POSIX sh only. Run it by hand, or through `mise run lua-test`.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

found=0
for f in extractor/test/*.lua; do
    [ -f "$f" ] || continue
    found=1
    printf '== %s\n' "$f"
    lua "$f"
done

[ "$found" -eq 1 ] || printf 'no offline tests yet\n'
