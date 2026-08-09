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
# The honest limitation, stated rather than hidden: an all-lowercase-letters secret
# with no digits slips through. Generated credentials essentially never look like that,
# and the alternative — flagging every lowercase word — is what produced the noise this
# replaces. Run with --self-test to see exactly which shapes are caught and which are not.

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

# Does this value look like a credential rather than an identifier or a word?
looks_like_a_credential() {
  local value="$1"
  [[ "$value" =~ [A-Z] && "$value" =~ [0-9] ]] && return 0
  [[ "$value" =~ ^[a-f0-9]{32,}$ ]] && return 0
  return 1
}

# Pull the assigned value out of a `path:line:content` match.
value_of() {
  sed -E 's/.*(api[_-]?key|secret|password|token|bearer)[[:space:]]*[=:][[:space:]]*["'"'"']?//I' <<<"$1" \
    | grep -oE '^[A-Za-z0-9/+_=-]+'
}

scan() {
  local -a findings=()
  local line value

  # `|| true`: git grep exits 1 when it finds nothing, which is the good case.
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    value="$(value_of "$line" || true)"
    [ -n "$value" ] || continue
    if looks_like_a_credential "$value"; then
      findings+=("$line")
    fi
  done < <(git grep -nEI "$NAME_PATTERN" -- "${EXCLUDES[@]}" || true)

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

  echo "Values that must NOT be flagged:"
  check no 'new_session_token'
  check no 'nicht-das-echte-geheimnis'
  check no 'abgelaufenes-token'
  check no 'your-password-here'
  check no 'change_me_before_deploying'

  echo
  if [ "$failures" -gt 0 ]; then
    echo "self-test: $failures failure(s)"
    return 1
  fi
  echo "self-test: all shapes classified correctly"
}

case "${1:-scan}" in
  --self-test) self_test ;;
  scan)        scan ;;
  *)           echo "usage: $0 [scan|--self-test]" >&2; exit 2 ;;
esac
