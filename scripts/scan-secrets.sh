#!/usr/bin/env bash
#
# Refuse to let a credential reach a repository that is mirrored publicly.
#
# ONE definition, called by `.forgejo/workflows/ci.yml`, `.github/workflows/ci.yml`
# and `just lint`, because a gate that exists in three slightly different forms is
# three gates with three different opinions.
#
# WHY THIS IS NOT JUST A GREP
# ---------------------------
# It was, and it failed on every single run. The pattern matched an assignment to a
# credential-shaped *name* and ignored what was assigned, so it flagged
#
#     let token = new_session_token();               <- a function call
#     client_secret: "nicht-das-echte-geheimnis"     <- German for "not-the-real-secret"
#     let token = "abgelaufenes-token";              <- German for "expired-token"
#
# A security gate that is always red is not a security gate. People stop reading it,
# and then the one real hit scrolls past with the noise. So the name test is only the
# first pass; the second pass asks whether the VALUE looks like a credential.
#
# The shape test: a real secret has entropy, an identifier and a German word do not.
# A hit is reported only if its value additionally
#   - contains an uppercase letter AND a digit  (covers almost every generated token,
#     API key, and base64 blob), or
#   - is 32 or more hex characters              (covers lowercase-only digests and keys)
#
# An entropy test false-NEGATIVES on exactly the credentials you most want caught: a
# lowercase passphrase has no uppercase and no digit. Two rules close that, and both
# bypass the entropy gate entirely:
#
#   - a known issuer prefix (`ghp_`, `AKIA`, `xoxb-`, `gw_`, …) is a credential by
#     construction, whatever it is followed by
#   - PEM key material is matched on its own, with no assignment and no name in front
#     of it, because a private key is never assigned to a variable called `secret`
#
# The honest limitation that remains: a lowercase secret with no digits and no known
# prefix still slips through. The alternative — flagging every lowercase word — is what
# produced the noise this replaces. Run with --self-test to see exactly which shapes are
# caught and which are not.

set -euo pipefail

# Paths whose matches are not credentials by construction. `docs/**` is prose that
# discusses credentials; this file necessarily contains every pattern it looks for.
readonly EXCLUDES=(
  ':!*.example'
  ':!docs/**'
  ':!.github/workflows/**'
  ':!.forgejo/workflows/**'
  ':!scripts/scan-secrets.sh'
)

# First pass. Deliberately broad: an assignment to a credential-shaped name.
readonly NAME_PATTERN='(api[_-]?key|secret|password|token|bearer)[[:space:]]*[=:][[:space:]]*["'"'"']?[A-Za-z0-9/+_=-]{16,}'

# Issuer prefixes that identify a credential on sight. These bypass the entropy test
# below, because a token is a token however low-entropy its tail happens to look.
# `gw_` is this project's own API token prefix (M14).
readonly ISSUER_PREFIXES='^(ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|AKIA|ASIA|xox[baprs]-|sk-|sk_live_|pk_live_|glpat-|gw_|hf_|npm_|dckr_pat_|eyJ)'

# Key material, matched with no assignment and no name in front of it: a private key is
# never assigned to a variable called `secret`, so the name-based pass cannot see it.
readonly KEY_MATERIAL='-----BEGIN [A-Z ]*PRIVATE KEY-----'

# Does this value look like a credential rather than an identifier or a word?
looks_like_a_credential() {
  local value="$1"
  [[ "$value" =~ $ISSUER_PREFIXES ]] && return 0
  [[ "$value" =~ [A-Z] && "$value" =~ [0-9] ]] && return 0
  [[ "$value" =~ ^[a-f0-9]{32,}$ ]] && return 0
  return 1
}

# Pull the assigned value out of a `path:line:content` match.
value_of() {
  sed -E 's/.*(api[_-]?key|secret|password|token|bearer)[[:space:]]*[=:][[:space:]]*["'"'"']?//I' <<<"$1" \
    | grep -oE '^[A-Za-z0-9/+_=-]+'
}

# Run one git-grep pass, distinguishing "found nothing" from "failed to run".
#
# git grep exits 0 on a match, 1 on no match, and >1 on an error. Collapsing those with
# `|| true` is how a broken pass reports a clean repository: the key-material pattern
# starts with `-`, git parsed it as an option, the usage message went to stderr, and the
# scan cheerfully printed "No credential-shaped values found." Hence `-e`, and hence
# this function refusing to treat an error as an absence.
grep_pass() {
  local pattern="$1" out status
  set +e
  out="$(git grep -nEI -e "$pattern" -- "${EXCLUDES[@]}" 2>&1)"
  status=$?
  set -e
  if [ "$status" -gt 1 ]; then
    echo "::error::the secret scan could not run — treating that as a failure, not a pass" >&2
    echo "$out" >&2
    # `return`, never `exit`. The caller reads this through a command substitution, which
    # is a subshell: an `exit` here would kill only that subshell, the caller would read
    # an empty result, find no findings, and report a clean repository. Fail-closed has
    # to survive the shell's own scoping to mean anything.
    return 2
  fi
  [ "$status" -eq 0 ] && printf '%s\n' "$out"
  return 0
}

scan_only() {
  local -a findings=()
  local line value name_hits key_hits

  # If the scan can see no files at all, something is wrong with the working
  # directory or the exclude list, and "no credentials found" would be true only
  # because nothing was looked at. That is the same false-clean failure as a broken
  # pattern, arriving by a different route.
  local visible
  visible="$(git ls-files -- "${EXCLUDES[@]}" | wc -l)"
  if [ "$visible" -eq 0 ]; then
    echo "::error::the scan matched no files at all — refusing to report a clean repository" >&2
    return 2
  fi

  name_hits="$(grep_pass "$NAME_PATTERN")" || return 2
  # Key material needs no name in front of it, so it is its own pass.
  key_hits="$(grep_pass "$KEY_MATERIAL")" || return 2

  while IFS= read -r line; do
    [ -n "$line" ] || continue
    value="$(value_of "$line" || true)"
    if [ -z "$value" ]; then
      # The first pass matched but the value could not be extracted, so the shape
      # test cannot be applied. Report it rather than dropping it: an unclassifiable
      # hit is the one case where a human must look, and silently skipping it is how
      # a scanner disagrees with itself and lets something through.
      findings+=("$line   [value could not be extracted — classify by hand]")
      continue
    fi
    if looks_like_a_credential "$value"; then
      findings+=("$line")
    fi
  done <<<"$name_hits"

  while IFS= read -r line; do
    [ -n "$line" ] && findings+=("$line")
  done <<<"$key_hits"

  if [ ${#findings[@]} -gt 0 ]; then
    printf '%s\n' "${findings[@]}"
    echo
    echo "::error::A credential-shaped value is committed. Rotate it first, then remove it."
    echo "Deleting the commit does not help: the mirror is public and the value must be"
    echo "treated as disclosed from the moment it was pushed."
    return 1
  fi

  echo "No credential-shaped values found."
}

self_test() {
  local failures=0
  check() { # check <should-flag: yes|no> <value>
    local want="$1" value="$2" got=no
    looks_like_a_credential "$value" && got=yes
    if [ "$got" != "$want" ]; then
      echo "  FAIL  expected=$want got=$got  for: $value"
      failures=$((failures + 1))
    else
      echo "  ok    $want  $value"
    fi
  }

  echo "Values that MUST be flagged:"
  check yes 'ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8'
  check yes 'AKIAIOSFODNN7EXAMPLE'
  check yes 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9'
  check yes '4f3c2b1a9e8d7c6b5a4938271605f4e3d2c1b0a99887766554433221100ffeedd'
  check yes 'S3cret-Value-With-Digits-42'
  # The entropy test alone would MISS every one of these — no uppercase, no digit.
  # They are caught by the issuer prefix, which is the point of having one.
  check yes 'ghp_alllowercasenodigitshere'
  check yes 'xoxb-slackbottokenlowercase'
  check yes 'glpat-alllowercasegitlabpat'
  check yes 'sk-livekeyallinlowercaseonly'

  echo "Values that must NOT be flagged:"
  check no 'new_session_token'
  check no 'nicht-das-echte-geheimnis'
  check no 'abgelaufenes-token'
  check no 'your-password-here'
  check no 'change_me_before_deploying'

  # Classifying correctly is not the same as SCANNING correctly. The pass that reads
  # the repository is a separate thing that can break on its own — and did, silently.
  # So plant known files in a throwaway repository and run the real scan against them.
  echo
  echo "End-to-end, in a throwaway repository:"
  local tmp original
  original="$PWD"
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064  # expand $tmp now, not at trap time
  trap "cd '$original' 2>/dev/null; rm -rf '$tmp'" RETURN

  git init -q "$tmp"
  printf 'let api_key = "sk-Ab3dEf9HiJ2kLm4NoP6qRs8TuV0wXy1Z";\n' > "$tmp/planted_key.rs"
  printf -- '-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaAo=\n' > "$tmp/planted_pem.txt"
  printf 'let token = new_session_token();\n' > "$tmp/innocent.rs"
  git -C "$tmp" add -A

  cd "$tmp"
  local output status
  set +e
  output="$(scan_only 2>&1)"
  status=$?
  set -e
  cd "$original"

  # `note <description> <0|1>`. Never `[ … ]; note "$?"` — under `set -e` a bare failing
  # test aborts the function, so a FAILING expectation would silently truncate the
  # report instead of printing FAIL. That happened here, which is the whole reason this
  # is written the long way.
  note() {
    if [ "$2" -eq 0 ]; then echo "  ok    $1"; else echo "  FAIL  $1"; failures=$((failures + 1)); fi
  }
  local rc

  rc=0; [ "$status" -eq 1 ] || rc=1
  note "a planted credential fails the scan (exit 1)" "$rc"
  rc=0; grep -q 'planted_key.rs' <<<"$output" || rc=1
  note "the issuer-prefixed key is reported" "$rc"
  rc=0; grep -q 'planted_pem.txt' <<<"$output" || rc=1
  note "PEM key material is reported" "$rc"
  rc=0; grep -q 'innocent.rs' <<<"$output" && rc=1
  note "the innocent identifier is not reported" "$rc"

  # A scan that CANNOT RUN must not report success. This is the case that got through
  # once already: the pass errored, the error was swallowed, and the repository was
  # declared clean. The directory must be outside any repository for git grep to fail,
  # so it cannot live under the throwaway repo above.
  local outside
  outside="$(mktemp -d)"
  cd "$outside"
  set +e
  scan_only >/dev/null 2>&1
  status=$?
  set -e
  cd "$original"
  rm -rf "$outside"
  rc=0; [ "$status" -eq 2 ] || rc=1
  note "a scan that cannot run fails (exit 2) rather than passing" "$rc"

  echo
  if [ "$failures" -gt 0 ]; then
    echo "self-test: $failures failure(s)"
    return 1
  fi
  echo "self-test: classification and scanning both correct"
}

# A clean report is only trustworthy if the scanner could still have failed, so
# proving that is part of producing it — not a separate step someone can drop from
# a workflow, and not a step that can pass in one job while the real scan runs
# broken in another. The self-test builds a throwaway repository and takes about a
# tenth of a second, which is not a price worth optimising away.
scan() {
  if ! self_test >/dev/null 2>&1; then
    echo "::error::the scanner failed its own self-test — its verdict means nothing" >&2
    echo "Run '$0 --self-test' to see which check failed." >&2
    return 2
  fi
  scan_only
}

case "${1:-scan}" in
  --self-test) self_test ;;
  scan)        scan ;;
  *)           echo "usage: $0 [scan|--self-test]" >&2; exit 2 ;;
esac
