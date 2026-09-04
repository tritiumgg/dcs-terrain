#!/bin/sh
# Stop: refuse to end a turn that changed the tree without stamping STATE.md.
#
# docs/STATE.md is updated at the end of every working session, before handing
# back, and its Last updated line carries the datetime. A session that changed
# something and left the file alone has lost that work, and nothing else
# enforces it. So when tracked files differ from HEAD and STATE.md is not
# stamped today, the stop is refused once, with the reason, and the model
# either updates the file or says in its reply why STATE.md does not change.
#
# The match is on the date, not the whole stamp: a session that already
# stamped an earlier hour today has done the thing this asks for.
#
# stop_hook_active is true when the model is already continuing because of
# this hook. The second stop passes, so the refusal cannot loop.
#
# Exit 2 refuses the stop and hands stderr to the model.

. "$(dirname -- "$0")/payload.sh"

active=$(parse stop_hook_active)
[ "$active" = "true" ] && exit 0

cd "$(project_root)" || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

changed=$(git status --porcelain 2>/dev/null)
[ -n "$changed" ] || exit 0

today=$(date -u +%Y-%m-%d)
grep -q "^\*\*Last updated:\*\* ${today}T" docs/STATE.md 2>/dev/null && exit 0

cat >&2 <<EOF
The working tree has changes and docs/STATE.md is not stamped $today.

STATE.md is the only record of where the work is, and it is updated at the end
of every working session, not only when a task finishes. Add what you did to
Done, move what you finished out of Next and refill Then, delete every carry
this session discharged, and set the Last updated line to today's UTC datetime:

    **Last updated:** $(date -u +%Y-%m-%dT%H:%MZ)

Or, where this turn changed nothing STATE.md should carry, say so in one line
of the reply and stop.
EOF
exit 2
