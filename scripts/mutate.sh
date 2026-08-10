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
readonly BACKUP_DIR=".mutate-backups"
readonly MARKER="$BACKUP_DIR/in-progress"

mkdir -p "$BACKUP_DIR"

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

  # The whole suite, unfiltered. `cargo test --workspace <filter>` silently runs ZERO
  # tests here — every binary reports "0 passed; N filtered out" — so filtering scored
  # every mutation as survived. Unfiltered is also more honest: a mutation caught by a
  # test in another crate is still caught.
  local_out="$(cargo test --workspace 2>&1 || true)"
  if grep -q "test result: FAILED" <<<"$local_out"; then
    outcome=killed
  elif grep -qE "^error(\[|:)" <<<"$local_out"; then
    # A mutation that does not compile tests nothing at all.
    outcome=uncompilable
  else
    outcome=survived
  fi

  restore "$file" "$backup"

  case "$outcome:$expectation" in
    uncompilable:*)
      echo "  ERROR    $description"
      echo "           the mutated code does not compile, so it exercises no test at all."
      unexpected=$((unexpected + 1)) ;;
    killed:killed)
      echo "  KILLED   $description"; killed=$((killed + 1)) ;;
    survived:equivalent)
      echo "  (equiv)  $description"; killed=$((killed + 1)) ;;
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

echo
if [ "$survived" -gt 0 ] || [ "$unexpected" -gt 0 ]; then
  echo "mutation testing: $killed as expected, $survived survived, $unexpected mis-recorded"
  exit 1
fi
echo "mutation testing: $killed mutations, all as expected"
