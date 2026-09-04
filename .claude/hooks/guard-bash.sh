#!/bin/sh
# PreToolUse guard for Bash: refuse the commands the project forbids, and hand
# the ones the maintainer decides to the maintainer.
#
# Two outcomes. A refusal exits 2 with the reason on stderr, and the model
# reads it and picks the replacement. An ask prints a permission decision on
# stdout and exits 0, so the harness prompts the person at the keyboard
# whatever the permission mode. That split follows CLAUDE.md: some things are
# wrong here and are not done, and some are the maintainer's call.
#
# Refused:
#   a write into the DCS install       it is read-only, always
#   a redirect, tee, cp, mv or rm      docs/spec/ is frozen
#     aimed at docs/spec/ or .gitattributes
#   cargo, lua, rustc, rustfmt bare    run through mise exec -- or mise run
#   rustup, mise use rust              the toolchain lives in rust-toolchain.toml
#   git merge without --ff-only        history is linear
#   git push --force                   --force-with-lease, on a topic branch
#   gh pr create without the           every pull request follows the template
#     template's four headings
#   sed -i, grep -P, readlink -f       not portable across the three platforms
#
# Asked:
#   git push to main, git tag, gh release, gh pr merge, and a rewrite of
#   main's history.

. "$(dirname -- "$0")/payload.sh"

tool=$(parse tool_name)
[ "$tool" = "Bash" ] || exit 0

cmd=$(parse command)
[ -n "$cmd" ] || exit 0

# Every line of the command that is not a comment.
lines=$(printf '%s\n' "$cmd" | grep -v '^[[:space:]]*#' || true)

has() {
    printf '%s\n' "$lines" | grep -Eq -e "$1"
}

refuse() {
    printf 'Blocked: %s\n\n%s\n' "$1" "$2" >&2
    exit 2
}

ask() {
    reason=$(printf '%s' "$1" | tr '\n' ' ' | sed 's/["\\]/\\&/g')
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":"%s"}}\n' "$reason"
    exit 0
}

# A word in command position: the start of a line or of a pipeline segment.
START='(^|[;&|(]|then|do|else)[[:space:]]*'
OPTS='(-[A-Za-z]*[[:space:]]+)*'
WRITER="${START}(cp|mv|rm|truncate|install|mkdir|touch)[[:space:]]"

# A redirect whose target matches $1. The target is checked quoted as well as
# bare, because every real DCS install path has spaces in it -- "Program
# Files", "Eagle Dynamics", "DCS World" -- so it only ever appears in quotes,
# and a bare-word pattern stops at the first space and misses it.
redirect_to() {
    has ">>?[[:space:]]*\"[^\"]*$1" && return 0
    has ">>?[[:space:]]*'[^']*$1" && return 0
    has ">>?[[:space:]]*[^[:space:];|&]*$1"
}

# Any write at that path: a redirect, a tee, or a command that creates or
# removes. Nothing here needs to know which one it was.
writes_to() {
    redirect_to "$1" || has "[[:space:]]tee[[:space:]].*$1" || has "${WRITER}.*$1"
}

# --- refusals --------------------------------------------------------------

# The DCS install is read-only. This is the rule with the worst failure mode
# in the project, so it is checked before anything else. Saved Games is not
# the install and stays writable: the hook is copied there by hand, and its
# directory is named DCS or DCS.openbeta rather than "DCS World".
INSTALL='(Eagle Dynamics|DCS World|autoupdate\.cfg)'
if writes_to "$INSTALL"; then
    refuse "a write into the DCS install." \
"The install is read-only. Nothing in this project writes to it, and a probe
that does can leave a theatre unloadable.

Read from it freely. What the hook needs goes in the Saved Games write
directory, which lfs.writedir() reports and which is not the install."
fi

FROZEN='(docs/spec/|\.gitattributes)'
if writes_to "$FROZEN"; then
    refuse "a shell write to a frozen document or to .gitattributes." \
"docs/spec/ is frozen: not edited, not renamed, not moved. Anything you would
have changed there goes in docs/decisions/ as an ADR instead, from
docs/decisions/TEMPLATE.md. .gitattributes keeps the ledger stamps valid
on a Windows checkout."
fi

if has "${START}(cargo|lua5\\.1|lua|luac|rustc|rustfmt)([[:space:]]|$)"; then
    refuse "a toolchain command outside mise." \
"A non-interactive shell does not pick up mise's PATH activation, so the tool
is the wrong version or missing. On Windows lua is the interpreter mise run
lua51 builds into .tools/bin, which only mise puts on PATH.

Run it as mise exec -- <command>, or as one of the tasks in mise.toml:
mise run check, mise run test, mise run lua-test, mise run docs."
fi

if has "${START}rustup([[:space:]]|$)|${START}mise[[:space:]]+use[[:space:]]+rust"; then
    refuse "a direct change to the Rust toolchain." \
"The Rust version, its profile and its components live in rust-toolchain.toml,
because rustup reads that file whether or not mise is installed. mise is told
to read it too, so Rust has one pin rather than two that can disagree.
mise use rust@ writes a second version into mise.toml and mise stops reading
the file. Edit channel in rust-toolchain.toml, then run mise install."
fi

if has "git[[:space:]]+merge([[:space:]]|$)" && ! has "git[[:space:]]+merge.*--ff-only"; then
    refuse "git merge without --ff-only." \
"History is linear. Bring a branch up to date with git rebase main; the
maintainer lands it with git merge --ff-only. If the fast-forward is refused,
fix the branch rather than reaching for a merge commit."
fi

if has "git[[:space:]]+push.*([[:space:]]--force|[[:space:]]-f)([[:space:]]|$)" && ! has '--force-with-lease'; then
    refuse "git push --force." \
"Use --force-with-lease, on a topic branch that is yours. Never on main."
fi

if has "gh[[:space:]]+pr[[:space:]]+create"; then
    body=$cmd
    file=$(printf '%s\n' "$lines" | sed -E -n 's/.*(--body-file|-F)[= ]*([^ ]*).*/\2/p' | head -1)
    if [ -n "$file" ]; then
        # A path the shell would expand, such as "$S/pr.md", is not readable
        # here, and a body that cannot be read cannot be judged.
        file=$(printf '%s' "$file" | tr -d '"'"'")
        [ -r "$file" ] || exit 0
        body=$(cat "$file")
    fi
    missing=""
    for h in Summary Details README Testing; do
        # Anywhere, not at line start: the body may arrive on one line with
        # literal \n sequences, or through a heredoc with real newlines.
        printf '%s\n' "$body" | grep -q "## $h" || missing="$missing $h"
    done
    if [ -n "$missing" ]; then
        refuse "a pull request body without the template's headings:$missing." \
"Every pull request body follows .github/PULL_REQUEST_TEMPLATE.md. gh pr
create --body does not read the template, so write the body to its headings
and put none under any that does not apply."
    fi
fi

if has "${START}sed[[:space:]]+${OPTS}-[A-Za-z]*i"; then
    refuse "sed -i." \
"Use the Edit tool. It verifies the match before writing, and BSD and GNU sed
disagree on what -i takes."
fi

if has "${START}grep[[:space:]]+${OPTS}-[A-Za-z]*P|--perl-regexp"; then
    refuse "grep -P." \
"BSD grep has no -P. Write the pattern for grep -E, or use rg."
fi

if has "${START}readlink[[:space:]]+-[A-Za-z]*f"; then
    refuse "readlink -f." \
"macOS has no readlink -f. Use CDPATH= cd -- \"\$(dirname -- \"\$f\")\" && pwd."
fi

# --- asks ------------------------------------------------------------------

RULE="CLAUDE.md: ask first before merging a pull request, before pushing main, before rewriting history that has been pushed, and before tagging."

branch=""
current_branch() {
    [ -n "$branch" ] && return
    branch=$(cd "$(project_root)" 2>/dev/null && git symbolic-ref --short -q HEAD 2>/dev/null || true)
}

if has "git[[:space:]]+push([[:space:]]|$)"; then
    # The non-option words after push. One or none means the current branch.
    args=$(printf '%s\n' "$lines" | grep -E 'git[[:space:]]+push' | head -1 \
        | sed 's/.*git[[:space:]]*push//' | tr ' ' '\n' | grep -v '^-' | grep -v '^$' || true)
    n=$(printf '%s\n' "$args" | grep -c . || true)
    if printf '%s\n' "$args" | grep -Eq '^(\+?main|[^:]*:main)$'; then
        ask "This pushes main. $RULE"
    fi
    if [ "$n" -le 1 ]; then
        current_branch
        [ "$branch" = "main" ] && ask "This pushes the current branch, which is main. $RULE"
    fi
fi

if has "git[[:space:]]+tag[[:space:]]+[^[:space:]]" \
    && ! has "git[[:space:]]+tag[[:space:]]+(-l|-n|--list|--contains|--points-at|--sort|--merged|--no-merged)"; then
    ask "This creates or deletes a tag, and a tag is a release. $RULE"
fi

if has "gh[[:space:]]+release[[:space:]]+(create|delete|edit|upload)"; then
    ask "This changes a GitHub release. $RULE"
fi

if has "gh[[:space:]]+pr[[:space:]]+merge"; then
    ask "This merges a pull request. $RULE"
fi

if has "git[[:space:]]+(rebase([[:space:]]|$)|commit.*--amend|reset[[:space:]]+.*--hard|push.*--force-with-lease)" \
    && ! has "git[[:space:]]+rebase[[:space:]]+--(continue|abort|skip)"; then
    current_branch
    [ "$branch" = "main" ] && ask "This rewrites main. $RULE"
fi

exit 0
