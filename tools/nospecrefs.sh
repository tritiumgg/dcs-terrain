#!/bin/sh
# Refuse a citation of docs/ from the code.
#
# The specifications are frozen, so they drift from the code as decisions
# accumulate. A citation is a claim that the named document still describes
# what the code does, and nothing checks that claim: it rots silently and
# points the next reader at text that has been superseded.
#
# Write the reason instead of the reference. A comment saying why the code is
# the way it is survives; a pointer to the paragraph that said so does not.
#
# A task id and the plan's "done test" are refused for the same reason. Tasks
# are ephemeral: the plan retires when the tools ship, and a comment reading
# "task X3" then points at nothing. A test comment says what the test proves.
#
# An ADR citation is allowed, and is the only one. An ADR is numbered, never
# renumbered, superseded rather than edited, and stays true as the specs age.
# Both spellings pass, because the records were once cited as ADR-NNNN.
#
# Everything under docs/ is exempt, and so is any README. Documents cite
# documents by design, and a README is navigation: CLAUDE.md says so of the
# root one, and tools/probe-theatre/README.md does the same job for its
# directory.
#
# POSIX sh only. Run it by hand, or through `mise run docs`.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

# The frozen documents by the bare name the specs use for each other, the
# plan's word for a completion condition, and a task or milestone id in
# citation position. A bare id such as X3 or C5b is deliberately not matched:
# it collides with ordinary identifiers, and a citation reads "task X3".
#
# The bare directory docs/spec/ is deliberately not matched either. What rots
# is a claim that a named document still describes the code; a hook that
# guards the directory, or a tool that reads it, has to name it, and that is
# structure rather than a citation. Naming a document is the citation, so
# docs/spec/core.md still fails, on core.md.
PATTERN='design-and-facts\.md|extract-format\.md|extractor-hook\.md'
PATTERN="$PATTERN|query-operations\.md|mcp-server\.md|using-the-data\.md"
PATTERN="$PATTERN|probe-log[-0-9.]*\.md"
PATTERN="$PATTERN|(^|[^a-zA-Z])core\.md|(^|[^a-zA-Z])validation\.md"
PATTERN="$PATTERN|[Dd]one test|[Tt]ask [A-Z][0-9]|(^|[^A-Za-z])MS[0-9]"

# git ls-files rather than find, so an untracked scratch file is not the thing
# that fails somebody's build.
#
# This script is exempt because a frozen document's name is its subject rather
# than its authority: it names the patterns it refuses, and would otherwise
# fail on its own test data.
EXEMPT='^docs/|(^|/)README\.md$|^CLAUDE\.md$'
EXEMPT="$EXEMPT|^tools/nospecrefs\.sh\$"
FILES=$(git ls-files | grep -vE "$EXEMPT")
[ -n "$FILES" ] || { printf 'no files to check. Is this a checkout?\n' >&2; exit 2; }

# grep exits 1 when it matches nothing, which is the passing case here.
HITS=$(printf '%s\n' "$FILES" | xargs grep -nE "$PATTERN" 2>/dev/null || true)

# An ADR citation is the one allowed reference, in either spelling.
HITS=$(printf '%s\n' "$HITS" | grep -vE 'ADR[- ][0-9][0-9][0-9][0-9]' || true)

if [ -z "$HITS" ]; then
    printf 'the code cites no document\n'
    exit 0
fi

printf '%s\n' "$HITS" >&2
printf '\n' >&2
printf 'The code cites a frozen document, a task or a done test. Say why the\n' >&2
printf 'code is the way it is instead, in the comment itself, and cite an ADR\n' >&2
printf 'by id where the reasoning is too long to sit in a comment.\n\n' >&2
printf 'Documents may cite documents: docs/, any README and CLAUDE.md are\n' >&2
printf 'exempt.\n' >&2
exit 1
