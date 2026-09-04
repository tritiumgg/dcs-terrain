#!/bin/sh
# Run each hook in .claude/hooks against a payload and check its exit code.
#
# A hook is a guard, and a guard that silently passes everything is worse than
# none. This feeds each one the payloads it exists to refuse, the ones it must
# let through, and the ones the maintainer decides, and fails on the first
# surprise.
#
# The hooks that read the repository run against this checkout, so the cases
# that depend on its state -- a dirty tree, the current branch -- are written
# to hold either way.
#
# POSIX sh only. Run it by hand, or through `mise run docs`.
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
HOOKS=.claude/hooks
export CLAUDE_PROJECT_DIR="$ROOT"

fail=0
n=0

# expect <code> <hook> <payload>
expect() {
    want=$1; hook=$2; payload=$3
    n=$((n + 1))
    printf '%s' "$payload" | sh "$HOOKS/$hook" >/dev/null 2>&1
    got=$?
    if [ "$got" -ne "$want" ]; then
        printf 'FAIL %-24s want %d got %d  %s\n' "$hook" "$want" "$got" "$payload" >&2
        fail=1
    fi
}

# expect_ask <hook> <payload>: exit 0 and a permission decision on stdout
expect_ask() {
    hook=$1; payload=$2
    n=$((n + 1))
    out=$(printf '%s' "$payload" | sh "$HOOKS/$hook" 2>/dev/null)
    got=$?
    case "$out" in
        *'"permissionDecision":"ask"'*) [ "$got" -eq 0 ] && return ;;
    esac
    printf 'FAIL %-24s want ask got %d  %s\n' "$hook" "$got" "$payload" >&2
    fail=1
}

# bash <command> -> a PreToolUse payload. The command is embedded as written,
# so a case needing a quote inside it writes \" the way the harness would.
bash_payload() {
    printf '{"tool_name":"Bash","tool_input":{"command":"%s"}}' "$1"
}
write_payload() {
    printf '{"tool_name":"Write","tool_input":{"file_path":"%s"}}' "$1"
}

refuse_bash() { expect 2 guard-bash.sh "$(bash_payload "$1")"; }
allow_bash()  { expect 0 guard-bash.sh "$(bash_payload "$1")"; }
ask_bash()    { expect_ask guard-bash.sh "$(bash_payload "$1")"; }

# --- every hook ignores a tool that is not its own -------------------------

expect 0 guard-bash.sh '{"tool_name":"Read","tool_input":{"file_path":"x"}}'
expect 0 guard-frozen-writes.sh '{"tool_name":"Bash","tool_input":{"command":"ls"}}'
expect 0 precommit.sh '{"tool_name":"Read","tool_input":{"file_path":"x"}}'
expect 0 postcommit.sh '{"tool_name":"Read","tool_input":{"file_path":"x"}}'

# --- guard-frozen-writes ---------------------------------------------------

expect 2 guard-frozen-writes.sh "$(write_payload 'docs/spec/core.md')"
expect 2 guard-frozen-writes.sh "$(write_payload 'docs/spec/extract-format.md')"
expect 2 guard-frozen-writes.sh "$(write_payload 'docs/spec/ledger/core-ledger.tsv')"
expect 2 guard-frozen-writes.sh "$(write_payload 'docs/spec//core.md')"
expect 2 guard-frozen-writes.sh "$(write_payload '/home/x/dcs-terrain/docs/spec/core.md')"
expect 2 guard-frozen-writes.sh "$(write_payload '.gitattributes')"

# A Windows path arrives with its separators escaped for JSON. Built here
# rather than written inline, because a doubled backslash in a shell literal
# is one backslash by the time the payload is assembled, and the case then
# silently stops being a Windows path.
BS=$(printf '\\')
WINSPEC="C:${BS}${BS}Users${BS}${BS}x${BS}${BS}docs${BS}${BS}spec${BS}${BS}core.md"
WINOK="C:${BS}${BS}Users${BS}${BS}x${BS}${BS}docs${BS}${BS}plan.md"
expect 2 guard-frozen-writes.sh "$(write_payload "$WINSPEC")"
expect 0 guard-frozen-writes.sh "$(write_payload "$WINOK")"

# Living documents are edited directly.
expect 0 guard-frozen-writes.sh "$(write_payload 'docs/STATE.md')"
expect 0 guard-frozen-writes.sh "$(write_payload 'docs/plan.md')"
expect 0 guard-frozen-writes.sh "$(write_payload 'docs/decisions/0010-a-new-record.md')"
expect 0 guard-frozen-writes.sh "$(write_payload 'docs/README.md')"
expect 0 guard-frozen-writes.sh "$(write_payload 'extractor/DcsTerrainExtract.lua')"
expect 0 guard-frozen-writes.sh '{"tool_name":"Write","tool_input":{}}'

# --- guard-bash: the DCS install is read-only ------------------------------
#
# Every real install path has spaces in it, so it only ever appears quoted.
# A bare-word pattern stops at the first space and misses all of these.

refuse_bash 'cp probe.lua \"C:/Program Files/Eagle Dynamics/DCS World/Scripts/x.lua\"'
refuse_bash 'echo hi > \"C:/Program Files/Eagle Dynamics/DCS World/Config/x.cfg\"'
refuse_bash "echo hi > 'C:/Program Files/Eagle Dynamics/DCS World/Config/x.cfg'"
refuse_bash 'echo hi >> \"D:/DCS World OpenBeta/Scripts/probe.lua\"'
refuse_bash 'cat x | tee \"C:/Program Files/Eagle Dynamics/DCS World/y\"'
refuse_bash 'rm autoupdate.cfg'
refuse_bash 'mv a.lua \"/c/DCS World/b.lua\"'

# Reading the install is the normal case and must stay free.
allow_bash 'ls \"C:/Program Files/Eagle Dynamics/DCS World\"'
allow_bash 'cat \"C:/Program Files/Eagle Dynamics/DCS World/autoupdate.cfg\"'
allow_bash 'grep -rn Terrain \"C:/Program Files/Eagle Dynamics/DCS World/Scripts\"'
# Saved Games is not the install: the hook is copied there by hand.
allow_bash 'cp extractor/DcsTerrainExtract.lua \"C:/Users/x/Saved Games/DCS/Scripts/Hooks/\"'

# --- guard-bash: the frozen documents --------------------------------------

refuse_bash 'echo x >> docs/spec/core.md'
refuse_bash 'cp /tmp/a docs/spec/core.md'
refuse_bash 'rm .gitattributes'
refuse_bash 'echo x > \"docs/spec/core.md\"'
allow_bash 'cat docs/spec/core.md'
allow_bash 'grep -rn nodata docs/spec/'
allow_bash 'sh tools/ledger.sh lint'

# --- guard-bash: the toolchain ---------------------------------------------

refuse_bash 'cargo test --workspace'
refuse_bash 'cargo fmt --all'
refuse_bash 'lua extractor/test/grid.lua'
refuse_bash 'rustc --version'
refuse_bash 'rustup update'
refuse_bash 'mise use rust@1.99'
allow_bash 'mise exec -- cargo test'
allow_bash 'mise run check'
allow_bash 'mise run lua-test'
allow_bash 'mise install'

# --- guard-bash: version control -------------------------------------------

refuse_bash 'git merge feature'
refuse_bash 'git push --force origin topic'
allow_bash 'git merge --ff-only build/guards'
allow_bash 'git push --force-with-lease origin topic'
allow_bash 'git status'
allow_bash 'git rebase main'
allow_bash 'git commit -m \"fix: a thing\"'

ask_bash 'git push origin main'
ask_bash 'git push origin HEAD:main'
ask_bash 'git tag v0.1.0'
ask_bash 'gh release create v0.1.0'
ask_bash 'gh pr merge 14 --merge'
# A tag listing is not a tag.
allow_bash 'git tag --list'

# --- guard-bash: portability -----------------------------------------------

refuse_bash 'sed -i s/a/b/ f.txt'
refuse_bash 'grep -P x f.txt'
refuse_bash 'readlink -f x'
allow_bash 'sed s/a/b/ f.txt'
allow_bash 'grep -E x f.txt'

# A comment naming a refused command is not that command.
allow_bash '# cargo test is refused here'

# --- guard-bash: the pull request template ---------------------------------

TMP=${TMPDIR:-/tmp}/hooktest.$$
mkdir -p "$TMP"
printf '## Summary\nx\n## Details\nx\n## README\nnone\n## Testing\nnone\n' > "$TMP/good.md"
printf '## Summary\nx\n## Details\nx\n' > "$TMP/bad.md"
allow_bash "gh pr create --body-file $TMP/good.md"
refuse_bash "gh pr create --body-file $TMP/bad.md"
refuse_bash 'gh pr create --title t --body \"just a sentence\"'
# A body the guard cannot read is a body it cannot judge.
allow_bash 'gh pr create --body-file \"$SCRATCH/pr.md\"'
allow_bash 'gh pr list'
rm -rf "$TMP"

# --- session-start ---------------------------------------------------------

expect 0 session-start.sh '{"source":"startup"}'
expect 0 session-start.sh '{"source":"resume"}'
expect 0 session-start.sh '{"source":"compact"}'

n=$((n + 1))
out=$(printf '{"source":"startup"}' | sh "$HOOKS/session-start.sh" 2>/dev/null)
case "$out" in
    *'## docs/STATE.md'*'## Working tree'*) ;;
    *) printf 'FAIL %-24s startup output lacks STATE.md or the working tree\n' session-start.sh >&2; fail=1 ;;
esac

n=$((n + 1))
out=$(printf '{"source":"compact"}' | sh "$HOOKS/session-start.sh" 2>/dev/null)
[ -z "$out" ] || { printf 'FAIL %-24s compact must print nothing\n' session-start.sh >&2; fail=1; }

# --- check-state-stamp -----------------------------------------------------
#
# Already continuing because of this hook: the second stop always passes, so
# the refusal cannot loop. This holds whether or not the tree is dirty, which
# is the only case that can be asserted without knowing the checkout's state.

expect 0 check-state-stamp.sh '{"stop_hook_active":true}'

# On a clean tree the hook says nothing; on a dirty one it depends on today's
# stamp. Both are 0 or 2 and never anything else.
n=$((n + 1))
printf '{"stop_hook_active":false}' | sh "$HOOKS/check-state-stamp.sh" >/dev/null 2>&1
got=$?
case "$got" in
    0|2) ;;
    *) printf 'FAIL %-24s want 0 or 2 got %d\n' check-state-stamp.sh "$got" >&2; fail=1 ;;
esac

# --- precommit and postcommit fire only on a commit ------------------------

expect 0 precommit.sh  '{"tool_name":"Bash","tool_input":{"command":"git status"}}'
expect 0 postcommit.sh '{"tool_name":"Bash","tool_input":{"command":"git status"}}'
expect 0 precommit.sh  '{"tool_name":"Bash","tool_input":{"command":"echo git commit"}}'

# A real commit payload runs the document tools against this checkout, which
# is expected to be clean, so it passes. If it does not, the tools themselves
# are reporting something and that is worth seeing here.
expect 0 precommit.sh '{"tool_name":"Bash","tool_input":{"command":"git commit -m x"}}'

# --- postcommit judges a subject -------------------------------------------
#
# In a throwaway repository, not this one. postcommit reads HEAD, and HEAD is
# not ours to choose: a pull request is built from a merge commit GitHub
# writes, whose subject is "Merge <sha> into <sha>" and follows no convention.
# Asserting against real history passed locally and failed in CI, which is a
# statement about the checkout rather than about the hook.

COMMIT='{"tool_name":"Bash","tool_input":{"command":"git commit -m x"}}'
REPO=${TMPDIR:-/tmp}/hooktest-commits.$$
rm -rf "$REPO"
if git init -q "$REPO" 2>/dev/null; then
    ( cd "$REPO" \
        && git config user.email hooktest@example.invalid \
        && git config user.name hooktest ) >/dev/null 2>&1

    # subject_case <want> <subject>
    subject_case() {
        n=$((n + 1))
        ( cd "$REPO" \
            && : > "f$n" && git add "f$n" \
            && git commit -q --no-gpg-sign -m "$2" ) >/dev/null 2>&1
        got=$(printf '%s' "$COMMIT" \
            | CLAUDE_PROJECT_DIR="$REPO" sh "$ROOT/$HOOKS/postcommit.sh" >/dev/null 2>&1; echo $?)
        if [ "$got" -ne "$1" ]; then
            printf 'FAIL %-24s want %d got %d  subject: %s\n' postcommit.sh "$1" "$got" "$2" >&2
            fail=1
        fi
    }

    subject_case 2 'made some changes'
    subject_case 2 'Merge 1a2b3c4 into 5d6e7f8'
    subject_case 2 'feat: a subject long enough to run past the seventy-two character limit set here'
    subject_case 0 'feat(hooks): check the commit message'
    subject_case 0 'fix: a thing'
    subject_case 0 'build(hooks)!: a breaking change'
    rm -rf "$REPO"
else
    printf 'skip postcommit subject cases: git init failed in %s\n' "$REPO" >&2
fi

# --- report ----------------------------------------------------------------

if [ "$fail" -ne 0 ]; then
    printf '\n%d hook cases, at least one failed\n' "$n" >&2
    exit 1
fi
printf '%d hook cases pass\n' "$n"
