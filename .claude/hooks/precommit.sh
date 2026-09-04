#!/bin/sh
# PreToolUse on Bash: before a git commit, run the checks CI would fail on.
#
# The document tools all run in CI. Running them here moves the failure from a
# CI round trip to the moment of the commit, where the fix is a single edit.
#
# Each is run only when something it reads has changed, because the ledger lint
# reads nine documents and 3315 rows and is not worth paying for on a commit
# that touches Lua. nospecrefs is cheap and scans everything, so it always runs.
#
# The files checked are the ones that differ from HEAD on disk, staged or not,
# plus untracked ones. What is on disk is what a commit records, whether the
# command stages it first or uses -a.
#
# Exit 2 blocks the commit and hands stderr to the model as the reason.

. "$(dirname -- "$0")/payload.sh"

tool=$(parse tool_name)
[ "$tool" = "Bash" ] || exit 0

cmd=$(parse command)
printf '%s\n' "$cmd" | grep -Eq 'git[[:space:]]+commit([[:space:]]|$)' || exit 0

cd "$(project_root)" || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

fail=0
report() {
    printf '%s\n' "$1" >&2
    fail=1
}

out=$(sh tools/nospecrefs.sh 2>&1) || report "$out"

changed=$( { git diff --name-only HEAD; git ls-files --others --exclude-standard; } 2>/dev/null | sort -u)

if printf '%s\n' "$changed" | grep -qx 'docs/STATE.md'; then
    out=$(sh tools/statecheck.sh 2>&1) || report "$out"
fi

# A frozen document should never reach a commit at all -- guard-frozen-writes
# refuses the edit -- but a shell redirect or a hand edit outside the session
# can still land one, and then the stamps are the only thing that notices.
if printf '%s\n' "$changed" | grep -q '^docs/spec/'; then
    out=$(sh tools/ledger.sh lint 2>&1) || report "$out"
fi

# The shell guard and the hook tests name the commands they refuse, so the
# text scan skips them. sh -n still runs on both.
BASHISMS='\[\[[[:space:]]|^[[:space:]]*local[[:space:]]|^#!/bin/bash|^[[:space:]]*function[[:space:]]|[A-Za-z_]=\(|(^|[;&|(]|[[:space:]])sed[[:space:]]+(-[A-Za-z]*[[:space:]]+)*-[A-Za-z]*i|(^|[;&|(]|[[:space:]])grep[[:space:]]+(-[A-Za-z]*[[:space:]]+)*-[A-Za-z]*P|readlink[[:space:]]+-[A-Za-z]*f'
for f in $(printf '%s\n' "$changed" | grep -E '\.sh$'); do
    [ -f "$f" ] || continue
    out=$(sh -n "$f" 2>&1) || report "$f does not parse as sh:
$out"
    case "$f" in
        .claude/hooks/guard-bash.sh|tools/hooktest.sh) continue ;;
    esac
    hits=$(grep -nE "$BASHISMS" "$f" || true)
    [ -z "$hits" ] || report "$f is not POSIX sh. CLAUDE.md, Portability, lists what is out:
$hits"
done

if [ "$fail" -ne 0 ]; then
    printf '\nBlocked: the commit would fail CI. Fix the above, then commit.\n' >&2
    exit 2
fi
exit 0
