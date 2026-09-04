#!/bin/sh
# PreToolUse guard: refuse an edit to a frozen document or to .gitattributes.
#
# Everything under docs/spec/ is frozen. Those documents were reviewed as one
# set, they cite each other by bare file name, and every row in ledger/ quotes
# a verbatim line from one of them. One edit silently breaks joins that nothing
# in this repository can rebuild. So the whole directory counts, ledger/ and
# glossaries included, not just the .md files.
#
# Where the build needs to go somewhere the specifications did not anticipate,
# the record of that is a decision record, not an edit.
#
# .gitattributes turns off line-ending conversion. Without it every ledger
# stamp reports a mismatch on a Windows checkout, against files nobody edited.
#
# The freeze is one directory, so it is named here rather than read out of an
# index file that would be one more thing to keep in step.
#
# Exit 2 blocks the tool call and hands stderr to the model as the reason.

. "$(dirname -- "$0")/payload.sh"

path=$(parse file_path)
[ -n "$path" ] || exit 0

norm=$(normalize_path "$path")

why=""
case "$norm" in
    */docs/spec/*|docs/spec/*)
        why="a frozen document" ;;
    */.gitattributes|.gitattributes)
        why=".gitattributes, which keeps the ledger stamps valid on Windows" ;;
esac

[ -n "$why" ] || exit 0

cat >&2 <<EOF
Blocked: a write to $norm, which is $why.

docs/spec/ is frozen: not edited, not renamed, not moved. Anything you would
have changed there goes in docs/decisions/ as an ADR instead, and the ADR wins
over the spec text. Copy docs/decisions/0000-template.md to the next free
number and add the row to docs/decisions/README.md, which is the index an ADR
is found by.

Propose the decision before writing it. An ADR records a choice that binds
later work; it is not a note to self.

Leave .gitattributes alone for the same reason a ledger is not hand-edited:
its stamp is a hash of the bytes on disk.
EOF
exit 2
