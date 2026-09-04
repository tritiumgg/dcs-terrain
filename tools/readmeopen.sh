#!/bin/sh
# List what README.md says is not built yet.
#
# The README is for a human who has never seen the project, and it must never
# describe something that does not work without saying so. This prints the
# sentences that say so, with their line numbers, so the session that finishes
# one of them knows which paragraph to rewrite.
#
# It fails nothing. An open claim is a fact about the build, not a defect in
# the file, and the honest banner at the top is the first thing it reports.
#
# POSIX sh and awk only. Run it by hand, or through `mise run docs`.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FILE=${1:-$ROOT/README.md}

[ -f "$FILE" ] || { printf 'no README at %s\n' "$FILE" >&2; exit 2; }

awk '
    # Matched against a lowercased copy, because the banner opens a sentence
    # and "Not usable yet" would otherwise read as unmarked.
    { sub(/\r$/, ""); lower = tolower($0) }
    /^#/ { heading = $0; sub(/^#+ /, "", heading) }
    lower ~ /not usable|not final|not yet|does not yet|has not started|planned|so far/ {
        line = $0; sub(/^[ \t]+/, "", line)
        printf "%4d  %-22s %s\n", NR, heading, line
        n++
    }
    END {
        if (n == 0) { printf "README.md marks nothing as open\n"; exit }
        printf "\n%d open claim%s. Each comes out with the task that settles it.\n", n, (n == 1 ? "" : "s")
    }
' "$FILE"
