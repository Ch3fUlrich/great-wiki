#!/usr/bin/env bash
#
# Mutation testing for the code where a wrong answer is a disclosure.
#
# WHY THIS EXISTS
# ---------------
# A passing test suite says the code does something. It does not say the tests would
# notice if it stopped. Every security bug this project has shipped so far was under a
# green suite, and each was found by breaking the code on purpose:
#
#   - `a_mismatched_state_is_refused` passed because the OIDC flow died at a LATER nonce
#     check. Disabling the CSRF defence entirely failed no test.
#   - a forged-session test whose store held no principals, so any token would have failed
#     and the test asserted nothing about forgery.
#   - `an_anonymous_caller_reads_nothing` passed because no grant matched ANY subject, so
#     deleting the authentication check from the audit reader still passed.
#
# All three looked completely reasonable. Nothing but a mutation exposes them, so the
# mutations live here rather than in someone's shell history.
#
# HOW TO READ A RESULT
# --------------------
#   KILLED   the suite noticed. That is the pass condition.
#   SURVIVED the code was broken and every test still passed. Either the mutation is
#            genuinely equivalent — in which case record it below as `equivalent` with the
#            reason — or a test is missing. Assume the second until you have shown the first.
#
# Usage:  scripts/mutate.sh            run every mutation
#         scripts/mutate.sh audit      run those whose description matches "audit"

set -euo pipefail
cd "$(dirname "$0")/.."

# Each mutation is a function call, not a delimited string. The first version of this
# script parsed `file|tests|expectation|expr|description` with IFS='|', and the very
# first permission-engine mutation contained `||` — which split into empty fields and
# fed sed a truncated expression. A separator that can appear in the data is not a
# separator.
#
# `expectation` is `killed` for a mutation the suite must catch, or `equivalent` for one
# it provably cannot because the change has no observable effect. An `equivalent` entry
# must carry its reason in the description — otherwise it is indistinguishable from a gap
# somebody gave up on.
MUTATIONS=()
mutation() { MUTATIONS+=("$1"$'\x1f'"$2"$'\x1f'"$3"$'\x1f'"$4"); }

# --- the audit log: who may read who-did-what -------------------------------------
mutation crates/gw-store/src/audit.rs killed \
  's/can(principal, Action::Admin, Visibility::Restricted/can(principal, Action::Read, Visibility::Restricted/' \
  'audit: reading the log needs admin, not merely read'
mutation crates/gw-store/src/audit.rs killed \
  's/if !principal.is_authenticated() {/if false {/' \
  'audit: the authentication check actually gates the read'
mutation crates/gw-store/src/audit.rs killed \
  's/>= crate::Baseline::Admin/>= crate::Baseline::Public/' \
  'audit: instance-wide entries need the admin baseline'
mutation crates/gw-store/src/audit.rs equivalent \
  's/WHERE path IS NOT NULL ORDER BY/ORDER BY/' \
  "audit: SQL predicate is defence in depth — the loop's own 'else continue' already skips unscoped entries"

# --- the permission engine: one can() decides everything ---------------------------
mutation crates/gw-auth/src/permission.rs killed \
  's/if !principal.is_authenticated() || !principal.active {/if false {/' \
  'permission: authentication and activity gate everything below'
mutation crates/gw-auth/src/permission.rs killed \
  's/if action == Action::Read \&\& visibility == Visibility::Internal {/if visibility == Visibility::Internal {/' \
  'permission: internal reach confers READ only, never write'
mutation crates/gw-auth/src/permission.rs killed \
  's/if action == Action::Read \&\& visibility == Visibility::Public {/if visibility == Visibility::Public {/' \
  'permission: a public document is readable, not writable'
mutation crates/gw-auth/src/permission.rs equivalent \
  's/Subject::Anyone => false, \/\/ already handled above/Subject::Anyone => true,/' \
  'permission: Anyone in the final loop is unreachable-by-value — the early pass already returned true for every Anyone grant that satisfies the action, so the survivors all fail satisfies() anyway'

# --- the admin API: who may administer what ----------------------------------------
#
# The first of these is the one that matters. `can()` answers an `Anyone` grant BEFORE it
# checks whether the caller is signed in — that is what a public share link is — so on a
# path carrying `anyone: admin` the engine alone would hand the ACL editor to an
# anonymous request. The gates check authentication first, and
# `an_anonymous_caller_is_refused_even_where_anyone_holds_admin` is what proves it: without
# that grant in the fixture, no subject would match anything and the test would pass with
# the check deleted.
mutation crates/gw-api/src/routes/admin.rs killed \
  's/if !principal.is_authenticated() || !principal.active {/if false {/' \
  'admin: the gates check authentication before any grant is consulted'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/if baseline >= Baseline::Admin {/if baseline >= Baseline::Public {/' \
  'admin: instance-wide operations need the admin baseline, not merely an account'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        >= Baseline::Admin$/        >= Baseline::Public/' \
  'admin: the instance-admin bypass on a path is not a bypass for everybody'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^    if can(&principal, Action::Admin, Visibility::Restricted, &grants) {/    if can(\&principal, Action::Read, Visibility::Restricted, \&grants) {/' \
  'admin: administering a space needs admin on it, not merely read'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        if can(&principal, Action::Admin, Visibility::Restricted, &grants) {/        if can(\&principal, Action::Read, Visibility::Restricted, \&grants) {/' \
  'admin: reading the audit log needs admin on a path, not merely read'

# --- audited mutations: the record and the change stand or fall together ------------
mutation crates/gw-store/src/admin.rs killed \
  's/if !written {/if false {/' \
  'admin store: a membership that changed nothing is not recorded as a change'
mutation crates/gw-store/src/admin.rs killed \
  's/if existing.is_some() {/if false {/' \
  'admin store: a grant that was already there is not recorded as a new one'
mutation crates/gw-store/src/admin.rs killed \
  's/if !removed {/if false {/' \
  'admin store: a removal that removed nothing is not reported or recorded as done'

# --- the administrative interlock: the instance always keeps one administrator -------
#
# The failure this guards is not a disclosure but a lock-out, and it is permanent: an
# administrator who deactivates the last active administrator — themselves included —
# leaves nobody able to reach the console, and great-wiki cannot promote anybody in
# Authelia to fix it (ADR 0002). There is no in-app recovery, so the refusal is the whole
# defence and it must be impossible to break quietly.
#
# The first two mutations are the interlock itself. The third and fourth are the two ways
# the COUNT can be wrong while the refusal still looks implemented: counting a deactivated
# account, and reading a promotion as if it applied to everybody rather than to the one
# principal it names.
mutation crates/gw-store/src/admin.rs killed \
  's/if !active \&\& !leaves_an_administrator(\&mut tx).await? {/if false {/' \
  'admin store interlock: deactivating the last active administrator is refused'
mutation crates/gw-store/src/admin.rs killed \
  's/if !admin \&\& !leaves_an_administrator(\&mut tx).await? {/if false {/' \
  'admin store interlock: demoting the last administrator is refused by the same rule'
mutation crates/gw-store/src/acl.rs killed \
  's/SELECT id, groups FROM principals WHERE active = 1/SELECT id, groups FROM principals/' \
  'acl interlock: the floor counts administrators who can still sign in'
mutation crates/gw-store/src/acl.rs killed \
  's/SELECT 1 FROM instance_admins WHERE principal_id = ?1/SELECT 1 FROM instance_admins/' \
  'acl interlock: a per-account promotion promotes ONE principal, never everybody'
mutation crates/gw-store/src/acl.rs killed \
  's/    if promoted(\&mut \*conn, principal_id).await? {/    if false {/' \
  'acl interlock: the per-account promotion confers the admin baseline with no groups'
mutation crates/gw-store/src/admin.rs killed \
  's/if !apply_instance_admin(\&mut tx, id, admin, actor).await? {/if false {/' \
  'admin store interlock: a promotion that changed nothing is not recorded as a change'
mutation crates/gw-store/src/principals.rs killed \
  's/ORDER BY last_active_at DESC, p.username ASC/ORDER BY last_active_at ASC, p.username ASC/' \
  'interlock candidates: the successors who have actually been here lately come first'
mutation crates/gw-store/src/principals.rs killed \
  's/^            if crate::acl::baseline_on(\&mut conn, \&id, \&groups).await? >= crate::Baseline::Admin {$/            if false {/' \
  'interlock candidates: somebody who already administers the instance is not a candidate'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        instance_admin,$/        instance_admin: body.admin,/' \
  'admin interlock: "still an administrator" is asked of baseline_for, not assumed from the request'

# --- default reach by visibility ---------------------------------------------------
mutation crates/gw-store/src/acl.rs killed \
  's/Visibility::Internal => baseline >= Baseline::Internal,/Visibility::Internal => true,/' \
  'acl: internal documents are not readable by everyone'
mutation crates/gw-store/src/acl.rs killed \
  's/Visibility::Restricted => baseline >= Baseline::Admin,/Visibility::Restricted => true,/' \
  'acl: restricted documents are not readable by everyone'

# --- crash recovery ------------------------------------------------------------------
#
# A trap does not survive SIGKILL, and a killed run leaves the mutated file in place.
# That happened: an interrupted run left `can()` with the read-only check deleted, which
# makes every public document world-writable, and it sat in the working tree looking like
# ordinary uncommitted work. `cargo test` caught it only because a test happened to cover
# it — nothing about the file said "this is a mutation".
#
# So the backup lives at a known path, and a marker records what is currently mutated.
# Any later run restores it first. The marker is the durable part; the trap is now only
# an optimisation for the ordinary case.
#
# NOTHING ELSE MAY WRITE TO THE TREE WHILE THIS RUNS — and the marker does not save you
# from that one. This script restores by copying the backup over the file; a `cargo fmt`
# started by somebody else reads the file while it is MUTATED and writes it back after the
# restore, reinstating the mutation under a clean marker and an exit code of zero. That
# happened, across six files at once, and it left three security rules disabled in the
# working tree looking like ordinary uncommitted work: the admin API's authentication
# checks, the interlock below, and `can()`'s read-only check on `internal` — which makes
# every internal document world-writable. The run reported KILLED for all of them.
#
# The tell is `ERROR ... the mutation changed nothing` on entries that passed yesterday:
# the sed found no target because the file was already mutated. If you see one, do not
# trust the summary — check `git diff` for one-line changes you did not make before
# running anything else.
#
# This script never runs `cargo fmt`, and must not be made to: formatting a deliberately
# mutated file is precisely the mechanism above, with this script as the other party.
readonly BACKUP_DIR=".mutate-backups"
readonly MARKER="$BACKUP_DIR/in-progress"
# The tree as it was before anything was touched. A hash would tell you something changed;
# a copy tells you that AND puts it back, which is the difference between a harness that
# reports a hole and one that closes it.
readonly PRISTINE_DIR="$BACKUP_DIR/pristine"
readonly MANIFEST="$BACKUP_DIR/pristine.sha256"

mkdir -p "$BACKUP_DIR"

# One name per path, used for both the rotating backup and the pristine copy, so the two
# can never disagree about which file they hold.
mangle() { echo "$1" | tr '/' '_'; }

if [ -f "$MARKER" ]; then
  echo "WARNING: a previous mutation run did not finish. Restoring before continuing."
  while IFS=$'\t' read -r mfile mbackup; do
    [ -n "$mfile" ] || continue
    if [ -f "$mbackup" ]; then
      cp "$mbackup" "$mfile"
      echo "  restored $mfile"
    else
      echo "  ERROR: $mfile was mutated but its backup $mbackup is gone."
      echo "         Check it by hand — `git diff $mfile` — before trusting this tree."
      exit 2
    fi
  done < "$MARKER"
  rm -f "$MARKER"
  echo
fi

filter="${1:-}"
killed=0 survived=0 unexpected=0

# HOW LONG THIS IS ALLOWED TO TAKE
# --------------------------------
# A gate too slow to run stops being run. This one got there: eighteen mutations, a whole
# `cargo test --workspace` each, and argon2id at Authelia's parameters inside that suite —
# over ten minutes, and it timed out rather than finishing. Two things fixed it, and only
# the second one is in this file.
#
# The first was the suite itself: the hashing cost is now a value on `AppState`, so tests
# hash at `HashingCost::CHEAP_FOR_TESTS` while the server hashes at Authelia's, and the two
# tests that are genuinely about cost ask for the production parameters by name. The suite
# went from 58 s to under 7 s, and this script inherits all of it.
#
# The second is the probe below. Most mutations are caught by the tests of the crate they
# were made in, and building *that crate's* tests is a third of the cost of building the
# workspace's. So each mutation gets one cheap, targeted run first.
#
# WHY A PROBE CANNOT CHANGE A VERDICT
# -----------------------------------
# It is not a filter and it never decides that something was NOT caught:
#
#   - KILLED is only ever recorded because a real test in real code actually failed. A
#     probe that fails has already proved the mutation is caught; running the rest of the
#     suite could add nothing but time.
#   - SURVIVED is only ever recorded after `cargo test --workspace`, unfiltered, has passed
#     in full — exactly the command this script used before, unchanged. A probe that passes
#     decides nothing at all; it falls through.
#
# So a wrong or stale probe costs a couple of seconds and cannot cost correctness. Prove it
# rather than believe it: `MUTATE_NO_PROBE=1 ./scripts/mutate.sh` skips the probe entirely,
# and the two outputs must be identical line for line.
#
# (Note `cargo test --workspace <filter>` is NOT how to narrow anything here: it silently
# runs ZERO tests — every binary reports "0 passed; N filtered out" — which once scored
# every mutation as survived. Per-crate `-p` invocations are the form that works.)
#
# WHY THIS IS NOT PARALLEL
# ------------------------
# The obvious next step is several mutations at once, each with its own
# `CARGO_TARGET_DIR`. It was measured and rejected: this workspace's debug target
# directory is over ten gigabytes and the disk has about four free — one extra worker does
# not fit, let alone four, and a run that fills the disk breaks every other build on the
# machine rather than only this one. Separate target directories would not make it safe in
# any case: the race that has actually bitten this script is two processes writing the same
# SOURCE file, which no amount of artefact separation separates.
probe_for() {
  case "$1" in
    crates/gw-store/*) echo "-p gw-store --lib" ;;
    crates/gw-auth/*) echo "-p gw-auth --lib" ;;
    crates/gw-core/*) echo "-p gw-core --lib" ;;
    # gw-api's route gates are covered by an integration test rather than by unit tests,
    # and `-p gw-api` on its own would build all six integration binaries — nearly the
    # whole workspace. This names the one that covers the admin routes. If that stops
    # being true the probe simply stops firing.
    crates/gw-api/src/routes/admin.rs) echo "-p gw-api --test admin" ;;
    *) echo "" ;;
  esac
}

# One `cargo test` invocation, classified. `test result: FAILED` is checked BEFORE the
# error pattern because cargo prints `error: test failed, to rerun pass ...` for an
# ordinary failing test, and reading that as "does not compile" would turn every kill into
# a mis-recording.
run_tests() {
  local out
  out="$(cargo test "$@" 2>&1 || true)"
  if grep -q "test result: FAILED" <<<"$out"; then
    echo killed
  elif grep -qE "^error(\[|:)" <<<"$out"; then
    echo uncompilable
  else
    echo passed
  fi
}

# Put the file back, and PROVE it went back. A restore that silently failed would leave
# broken security code in the tree with a green summary printed over it.
restore() {
  local file="$1" backup="$2"
  cp "$backup" "$file"
  if ! cmp -s "$file" "$backup"; then
    echo "FATAL: could not restore $file from $backup. The tree is left mutated."
    exit 2
  fi
  rm -f "$MARKER"
  trap - EXIT INT TERM HUP
}

# Every file the selected mutations will touch, once each.
selected_files() {
  local entry f d
  for entry in "${MUTATIONS[@]}"; do
    IFS=$'\x1f' read -r f _ _ d <<<"$entry"
    [ -z "$filter" ] || [[ "$d" == *"$filter"* ]] || continue
    echo "$f"
  done | sort -u
}

# Is the tree we are handing back the tree we were given?
#
# `restore` proving the copy landed is NOT enough, because the damage arrives AFTER it. A
# concurrent `cargo fmt` reads a file while it is mutated and writes its buffer back once
# the restore has already happened and the marker has already been cleared — so the
# mutation is reinstated under a green summary and an exit code of zero. That is not a
# story: it happened twice, and the second time it left `can()` treating every internal
# document as writable, in a working tree that looked like ordinary uncommitted work.
#
# So the run is only over when every file it touched is byte-for-byte what it was. This is
# called after every restore, which localises the damage to one mutation's window, and
# again at the end, which is what makes the exit code mean something.
verify_tree() {
  local file pristine damaged=()
  while IFS= read -r file; do
    pristine="$PRISTINE_DIR/$(mangle "$file")"
    cmp -s "$file" "$pristine" || damaged+=("$file")
  done < <(selected_files)
  [ ${#damaged[@]} -eq 0 ] && return 0

  echo
  echo "FATAL: the working tree was modified underneath this run."
  echo
  echo "  These files no longer match what they were before it started:"
  for file in "${damaged[@]}"; do
    echo "    $file"
  done
  echo
  echo "  Every result printed above is void. The usual cause is another process writing"
  echo "  to the tree — a \`cargo fmt\` that read a file while it was MUTATED and wrote it"
  echo "  back after the restore, which reinstates the mutation silently. Whatever the"
  echo "  cause, a security rule may be disabled in your working tree right now."
  echo
  for file in "${damaged[@]}"; do
    cp "$PRISTINE_DIR/$(mangle "$file")" "$file"
    echo "  restored $file to its pre-run contents"
  done
  echo
  echo "  Check \`git diff\` before running anything else, then re-run this with nothing"
  echo "  else touching the repository."
  rm -f "$MARKER"
  exit 3
}

# Everything that has to be true before a single mutation is worth making.
preflight() {
  local file entry expr d stale=() missing=()

  # 1. Nobody else may be building or formatting this tree. Separate target directories do
  #    not help: the race is on the SOURCE file, not on the artefacts. `cargo fmt` is the
  #    dangerous one, but any cargo invocation can pull in a formatter or a fix-up, and a
  #    second `cargo test` would also fight this one for the target directory lock and make
  #    every timing here meaningless.
  #    `-x` matches the process NAME, not the command line. `-f` looks tempting and is
  #    wrong: it matches any shell, editor or tmux server whose arguments merely mention
  #    cargo, and a check that cries wolf is a check somebody switches off.
  local others
  others="$(pgrep -a -x 'cargo|rustc|rustdoc|rustfmt|cargo-clippy|cargo-fmt' 2>/dev/null || true)"
  if [ -n "$others" ] && [ -z "${MUTATE_ALLOW_CONCURRENT_CARGO:-}" ]; then
    echo "REFUSING TO START: another cargo or rustc is running against this machine."
    echo
    echo "$others" | sed 's/^/    /'
    echo
    echo "  This script rewrites security-critical source files in place and restores them"
    echo "  afterwards. Anything else holding those files open — a \`cargo fmt\` above all —"
    echo "  can write its copy back AFTER the restore and reinstate a mutation under a"
    echo "  green summary. Wait for the other build to finish."
    echo
    echo "  If that listing is a false positive (an editor's language server, say), re-run"
    echo "  with MUTATE_ALLOW_CONCURRENT_CARGO=1 — and then read the final integrity check"
    echo "  rather than the summary."
    exit 2
  fi

  # 2. Every target has to exist, and every sed expression has to still find something. A
  #    mutation whose target text is gone is either an entry that has rotted or — the
  #    reason this is checked up front rather than discovered halfway through — a file that
  #    is ALREADY mutated from an earlier interrupted or sabotaged run.
  for entry in "${MUTATIONS[@]}"; do
    IFS=$'\x1f' read -r file _ expr d <<<"$entry"
    [ -z "$filter" ] || [[ "$d" == *"$filter"* ]] || continue
    if [ ! -f "$file" ]; then
      missing+=("$file")
    elif sed "$expr" "$file" | cmp -s - "$file"; then
      stale+=("$d")
    fi
  done

  if [ ${#missing[@]} -gt 0 ]; then
    echo "REFUSING TO START: a mutation names a file that is not there."
    printf '    %s\n' "${missing[@]}"
    exit 2
  fi

  if [ ${#stale[@]} -gt 0 ]; then
    echo "WARNING: ${#stale[@]} mutation(s) will find nothing to change:"
    printf '    %s\n' "${stale[@]}"
    echo
    echo "  Either the code they target has moved, or the file is already mutated. Check"
    echo "  \`git diff\` for one-line changes you did not make before trusting this run."
    echo "  They are reported as errors below and the exit code will be non-zero."
    echo
  fi

  # 3. The suite must pass BEFORE anything is broken. This one is not hygiene: a mutation
  #    is scored KILLED because a test failed, so a suite that is ALREADY failing scores
  #    every mutation as killed no matter what — the harness would report a perfect run
  #    while proving nothing at all. That is the exact failure this script exists to catch,
  #    committed by the script itself.
  echo "checking the suite passes before anything is mutated..."
  case "$(run_tests --workspace)" in
    passed) ;;
    *)
      echo "REFUSING TO START: \`cargo test --workspace\` does not pass on the unmutated tree."
      echo
      echo "  Every mutation would be scored KILLED by the failure that is already there,"
      echo "  and the run would report a clean sweep having tested nothing. Fix the suite"
      echo "  first."
      exit 2
      ;;
  esac

  # 4. The tree as handed to us, kept for comparison and for putting things back.
  mkdir -p "$PRISTINE_DIR"
  rm -f "$MANIFEST"
  while IFS= read -r file; do
    cp "$file" "$PRISTINE_DIR/$(mangle "$file")"
  done < <(selected_files)
  if command -v sha256sum >/dev/null 2>&1; then
    # For the record and for a human reading afterwards. `verify_tree` compares against the
    # copies rather than these digests: a copy detects the same change and can also undo it.
    # shellcheck disable=SC2046
    sha256sum $(selected_files) > "$MANIFEST" 2>/dev/null || true
  fi
}

preflight

for entry in "${MUTATIONS[@]}"; do
  IFS=$'\x1f' read -r file expectation expr description <<<"$entry"
  [ -z "$filter" ] || [[ "$description" == *"$filter"* ]] || continue

  backup="$BACKUP_DIR/$(echo "$file" | tr '/' '_')"
  cp "$file" "$backup"
  printf '%s\t%s\n' "$file" "$backup" > "$MARKER"
  # Covers the ordinary interruptions. SIGKILL is covered by the marker above.
  # shellcheck disable=SC2064
  trap "cp '$backup' '$file'; rm -f '$MARKER'" EXIT INT TERM HUP

  sed -i "$expr" "$file"
  if cmp -s "$file" "$backup"; then
    echo "  ERROR    $description"
    echo "           the mutation changed nothing — the code it targets has moved or been"
    echo "           rewritten, so this entry is testing an assumption that no longer holds."
    unexpected=$((unexpected + 1))
    restore "$file" "$backup"
    continue
  fi

  started=$SECONDS
  outcome=""

  # Cheap and targeted, and only ever able to end this early by finding a real failure.
  probe="$(probe_for "$file")"
  if [ -z "${MUTATE_NO_PROBE:-}" ] && [ -n "$probe" ]; then
    # shellcheck disable=SC2086
    case "$(run_tests $probe)" in
      killed) outcome=killed ;;
      uncompilable) outcome=uncompilable ;;
    esac
  fi

  # The whole suite, unfiltered — the only thing allowed to conclude that a mutation was
  # NOT caught. A mutation killed by a test in another crate is still killed, so nothing
  # here is narrowed by crate, by name, or by anything else.
  if [ -z "$outcome" ]; then
    outcome="$(run_tests --workspace)"
    [ "$outcome" = passed ] && outcome=survived
  fi

  restore "$file" "$backup"
  # Immediately, so that damage is attributed to the window it landed in rather than
  # discovered at the end with twenty-six other mutations to disentangle it from.
  verify_tree
  elapsed=$((SECONDS - started))

  case "$outcome:$expectation" in
    uncompilable:*)
      echo "  ERROR    $description"
      echo "           the mutated code does not compile, so it exercises no test at all."
      unexpected=$((unexpected + 1)) ;;
    killed:killed)
      echo "  KILLED   [${elapsed}s] $description"; killed=$((killed + 1)) ;;
    survived:equivalent)
      echo "  (equiv)  [${elapsed}s] $description"; killed=$((killed + 1)) ;;
    survived:killed)
      echo "  SURVIVED $description"
      echo "           The code was broken and every test still passed. Add a test, or"
      echo "           demonstrate the mutation is equivalent and record it as such."
      survived=$((survived + 1)) ;;
    killed:equivalent)
      echo "  ERROR    $description"
      echo "           recorded as equivalent but the suite killed it — the note is wrong."
      unexpected=$((unexpected + 1)) ;;
  esac
done

# The last word, and it is about the tree rather than about the mutations: a summary
# printed over a working tree with a security rule disabled in it is worse than no summary.
verify_tree

echo
# The wall clock is part of the result. This gate is only useful if it is actually run,
# and the last time it stopped being run it was because it had quietly grown past ten
# minutes — which nothing in its own output said.
printf 'mutation testing: %dm %02ds total\n' $((SECONDS / 60)) $((SECONDS % 60))
if [ "$survived" -gt 0 ] || [ "$unexpected" -gt 0 ]; then
  echo "mutation testing: $killed as expected, $survived survived, $unexpected mis-recorded"
  exit 1
fi
echo "mutation testing: $killed mutations, all as expected"
