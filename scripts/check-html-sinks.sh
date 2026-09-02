#!/usr/bin/env bash
#
# Refuse to let stored content reach the DOM as markup.
#
# ONE definition, called by `.forgejo/workflows/ci.yml`, `.github/workflows/ci.yml`
# and `just lint`, for the reason `scan-secrets.sh` gives: a gate that exists in three
# slightly different forms is three gates with three different opinions.
#
# WHAT THIS PROTECTS
# ------------------
# `BlockView.svelte` renders a document by matching on block kind, and a kind it does
# not know is skipped rather than emitted raw. Its own comment states the consequence:
# *"no untrusted HTML is ever constructed"* — which is why there is no sanitisation
# step anywhere in the reader, and why an SVG attachment is safe to show through an
# `<img>` (ADR 0014). That sentence is load-bearing for a wiki whose pages are written
# by one person and read by another, and nothing but this check keeps it true.
#
# It is checked here rather than left to review because the next few pieces of work all
# push against it: a diagram renderer that hands back SVG, a formula typesetter that
# hands back HTML, a syntax highlighter that hands back markup. Each one arrives with an
# obvious two-character way to put its output on the page.
#
# WHY THE WHOLE SINK CLASS, NOT TWO SPELLINGS
# -------------------------------------------
# A check that greps for `{@html` and `innerHTML` teaches the next implementer to use
# `insertAdjacentHTML` or `setHTMLUnsafe` instead — same effect, no warning. So every
# spelling that parses a string as markup is listed, plus `eval(` and `new Function`,
# which are the same failure by another route.
#
# "Every spelling" is a claim, and it has been attacked once already: a review found two
# Svelte-native ways around the first version's fixed strings, one of which was real. The
# pattern list below records what was found and what was verified against the compiler,
# rather than restating the claim.
#
# And one that is not in our source at all: `mermaid.render(id, text, container)`.
# Handing that function a container makes IT perform the write on your behalf, so the
# sink is inside a dependency and no grep for our own spellings would ever see it. Hence
# a hand-written grep checking a library's call signature, which looks odd and is
# deliberate: the two-argument form returns a string and is the shape D-19 requires.
#
# WHAT IT DOES NOT COVER, HONESTLY
# --------------------------------
#  - Tests are not scanned. `BlockView.test.ts` asserts that these very strings are
#    ABSENT from rendered output, so it necessarily contains one of them; a test does
#    not ship in the page. That is a class exclusion, not an exemption — the named
#    exemption list below is empty and says what would be allowed to join it.
#  - Only TRACKED files are read, because this is `git grep` (as the secret scan is). A
#    brand-new file is invisible until it is `git add`ed, which is before it can be
#    committed and therefore before it can be reviewed or deployed.
#  - A three-argument `render(` split across lines slips through, as does a sink
#    assembled from string fragments. This is a grep, and it is the second line: the
#    Content-Security-Policy (`docs/decisions/0007-content-security-policy.md`) is the
#    first, and the reason there is no `unsafe-inline` in `script-src`.

set -euo pipefail

# Where the application's own source lives. Not `web/scripts` (a Playwright harness that
# reads a live page's `innerHTML`, which is an inspection and not a write, and never
# reaches a browser as our code) and not `web/build` (generated).
readonly SCOPE=('web/src' ':!*.test.ts' ':!*.spec.ts')

# Every spelling that turns a string into markup or into code, as extended regular
# expressions (`git grep -E`).
#
# The first version of this file used FIXED strings, on the argument that "an escaping
# mistake in a regex would make the gate quietly stop matching". A review took that
# argument apart and it does not survive: the self-test below plants one file per pattern
# and names the pattern that stops matching, which is precisely the escaping mistake the
# fixed strings were protecting against — so the protection was already there, and being
# unable to express "optional whitespace" cost real coverage. Two of the three findings
# below were reachable only because these were fixed strings.
#
#  - `innerHTML` and `outerHTML` carry NO leading dot. `.innerHTML` missed
#    `<div contenteditable bind:innerHTML={svg}>`, which is not exotic — it is Svelte's
#    own spelling, and it is a genuine sink at BOTH ends: `svelte@5.56.8` compiles it to
#    `$.bind_content_editable('innerHTML', div, svg)` on the client and, on the server,
#    to a bare `$$renderer.push(\`${svg}\`)` — the value interpolated into the SSR
#    response with no escaping at all. Verified by compiling it. Dropping the dot also
#    catches `el['innerHTML'] = svg` and `Object.assign(el, { innerHTML: svg })`.
#  - `\{[[:space:]]*@html` rather than `{@html`. **This one is belt and braces, and the
#    reason is worth recording so nobody deletes it as redundant.** The review's example
#    was `{ @html svg }`, one space after the brace, claimed to compile identically —
#    it does NOT: `svelte@5.56.8` rejects every whitespace variant with *"Expected a `@`
#    character immediately following the opening bracket"* (`block_unexpected_character`),
#    so today such a file cannot reach a browser, because it cannot build. The pattern
#    stays anyway: it costs one character class, it makes the claim in the section above
#    true of the SPELLING rather than of one compiler's parser, and the day that parser
#    relaxes, nothing else here would have noticed.
#  - `parseFromString` is the same class arriving by another route:
#    `new DOMParser().parseFromString(svg, 'text/html')` yields an inert document, but
#    the moment its nodes are adopted into the live one their event-handler attributes
#    are live too.
readonly SINKS=(
  '\{[[:space:]]*@html'
  'innerHTML'
  'outerHTML'
  'insertAdjacentHTML'
  'setHTMLUnsafe'
  'createContextualFragment'
  'parseFromString'
  'document\.write'
  'srcdoc'
  'new Function'
  'eval\('
)

# A `render(…, …, …)` call in a file that mentions mermaid: the three-argument form,
# which writes into the container it is given. Two stages rather than one pattern so
# that a destructured `import { render } from 'mermaid'` is caught as well as
# `mermaid.render`, without flagging every three-argument call in the codebase.
readonly MERMAID_CALL='render\([^)]*,[^)]*,'

# Files allowed to hold one of the above, each with the reason it is safe.
#
# EMPTY, and that is the true state of the tree: the reader constructs no HTML today.
# One entry is expected — the KaTeX leaf component, which renders a formula to HTML
# during SSR — and it must arrive in the SAME commit as the file it exempts, with a doc
# comment on that component saying what makes its input safe. An exemption added ahead
# of its file is a hole nobody is looking at.
readonly EXEMPT=()

# Run one git-grep pass, distinguishing "found nothing" from "failed to run".
#
# git grep exits 0 on a match, 1 on no match, and >1 on an error. Collapsing those with
# `|| true` is how a broken pass reports a clean tree — the failure `scan-secrets.sh`
# documents having shipped once already.
grep_pass() {
  local out status
  set +e
  out="$(git grep -nEI "$@" -- "${SCOPE[@]}" 2>&1)"
  status=$?
  set -e
  if [ "$status" -gt 1 ]; then
    echo "::error::the HTML-sink check could not run — treating that as a failure, not a pass" >&2
    echo "$out" >&2
    # `return`, never `exit`: the caller reads this through a command substitution, which
    # is a subshell, and an `exit` there would leave the caller finding no hits and
    # reporting a clean tree.
    return 2
  fi
  [ "$status" -eq 0 ] && printf '%s\n' "$out"
  return 0
}

# Is this `path:line:text` finding in an exempted file?
is_exempt() {
  local path="${1%%:*}" entry
  for entry in ${EXEMPT[@]+"${EXEMPT[@]}"}; do
    [ "$path" = "$entry" ] && return 0
  done
  return 1
}

scan_only() {
  local -a findings=()
  local line hits file visible sink_args=()

  # A check that looked at no files at all would report "clean" for the wrong reason —
  # a moved directory, a broken pathspec, the wrong working directory.
  visible="$(git ls-files -- "${SCOPE[@]}" | wc -l)"
  if [ "$visible" -eq 0 ]; then
    echo "::error::the HTML-sink check matched no files at all — refusing to report a clean tree" >&2
    return 2
  fi

  for sink in "${SINKS[@]}"; do sink_args+=(-e "$sink"); done
  hits="$(grep_pass "${sink_args[@]}")" || return 2
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    is_exempt "$line" || findings+=("$line")
  done <<<"$hits"

  # The dependency's own sink, file by file: only where mermaid is mentioned at all.
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    grep -qi 'mermaid' "$file" || continue
    while IFS= read -r line; do
      [ -n "$line" ] || continue
      is_exempt "$file:$line" || findings+=("$file:$line   [three-argument mermaid render]")
    done < <(grep -nE "$MERMAID_CALL" "$file" || true)
  done < <(git ls-files -- "${SCOPE[@]}")

  if [ ${#findings[@]} -gt 0 ]; then
    printf '%s\n' "${findings[@]}"
    echo
    echo "::error::Stored content would reach the DOM as markup."
    echo "The reader renders a document by matching on block kind and skips what it does"
    echo "not know, which is why nothing here sanitises anything. Keep it that way: return"
    echo "a STRING and put it in a text position, or an attribute (an <img src>), rather"
    echo "than parsing it as markup. If a component genuinely must render generated HTML,"
    echo "confine it to one leaf, document what makes its input safe, and add that file to"
    echo "EXEMPT in this script in the same commit."
    return 1
  fi

  echo "No HTML sinks in ${SCOPE[0]} ($visible files scanned)."
}

self_test() {
  local failures=0 original tmp output status rc
  original="$PWD"
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064  # expand $tmp now, not at trap time
  trap "cd '$original' 2>/dev/null; rm -rf '$tmp'" RETURN

  git init -q "$tmp"
  mkdir -p "$tmp/web/src/lib/components"
  local dir="$tmp/web/src/lib/components"

  # One planted file per spelling, so a pattern that stops matching is named rather
  # than merely missed.
  printf '<p>{@html gefaehrlich}</p>\n'                        > "$dir/planted_html.svelte"
  printf 'el.innerHTML = svg;\n'                               > "$dir/planted_inner.ts"
  printf 'el.outerHTML = svg;\n'                               > "$dir/planted_outer.ts"
  printf "el.insertAdjacentHTML('beforeend', svg);\n"          > "$dir/planted_adjacent.ts"
  printf 'el.setHTMLUnsafe(svg);\n'                            > "$dir/planted_unsafe.ts"
  printf 'range.createContextualFragment(svg);\n'              > "$dir/planted_fragment.ts"
  printf 'document.write(svg);\n'                              > "$dir/planted_write.ts"
  printf '<iframe srcdoc={svg}></iframe>\n'                    > "$dir/planted_srcdoc.svelte"
  printf 'const f = new Function(quelle);\n'                   > "$dir/planted_function.ts"
  printf 'const v = eval(quelle);\n'                           > "$dir/planted_eval.ts"
  # The four a review got past the first version of this file. Each one is here so that
  # narrowing a pattern back — restoring the leading dot on `innerHTML`, say, or dropping
  # the whitespace class from the `@html` pattern — fails by name instead of quietly
  # reopening the hole.
  printf '<div contenteditable bind:innerHTML={svg}></div>\n'  > "$dir/planted_bind.svelte"
  printf "el['innerHTML'] = svg;\n"                            > "$dir/planted_bracket.ts"
  printf 'Object.assign(el, { innerHTML: svg });\n'            > "$dir/planted_assign.ts"
  printf "const d = new DOMParser().parseFromString(svg, 'text/html');\n" \
    > "$dir/planted_parser.ts"
  # Whitespace after the brace. `svelte@5.56.8` refuses to compile this, so it is not a
  # live hole today — see SINKS. Planted because the pattern that covers it is the kind a
  # later tidy-up removes for looking redundant.
  printf '<p>{ @html gefaehrlich }</p>\n'                      > "$dir/planted_spaced.svelte"
  # The sink that is not ours: mermaid writing into a container it was handed.
  printf "import mermaid from 'mermaid';\nawait mermaid.render(id, quelle, ziel);\n" \
    > "$dir/planted_mermaid.ts"

  # …and what must NOT be flagged.
  printf "import mermaid from 'mermaid';\nconst { svg } = await mermaid.render(id, quelle);\n" \
    > "$dir/innocent_mermaid.ts"
  printf '<pre><code>{codeText(block)}</code></pre>\n'         > "$dir/innocent_reader.svelte"
  printf "const dritte = zeichne(a, b, c);\n"                  > "$dir/innocent_three_args.ts"
  # The safe half of the same Svelte pair, and the reason `innerHTML` lost its dot rather
  # than `bind:` being banned: `bind:textContent` compiles to `$.escape(...)` on the
  # server and sets `textContent` on the client. It is a text position, not a sink.
  printf '<div contenteditable bind:textContent={text}></div>\n' \
    > "$dir/innocent_textcontent.svelte"
  # A test asserting the sinks are ABSENT — the one real hit in this repository today,
  # and the reason tests are out of scope rather than exempted by name.
  printf "for (const forbidden of ['srcdoc', '<script']) expect(out).not.toContain(forbidden);\n" \
    > "$dir/BlockView.test.ts"

  git -C "$tmp" add -A
  cd "$tmp"
  set +e
  output="$(scan_only 2>&1)"
  status=$?
  set -e
  cd "$original"

  # `note <description> <0|1>`. Never `[ … ]; note "$?"` — under `set -e` a bare failing
  # test aborts the function, which would truncate the report instead of printing FAIL.
  note() {
    if [ "$2" -eq 0 ]; then echo "  ok    $1"; else echo "  FAIL  $1"; failures=$((failures + 1)); fi
  }

  echo "Files that MUST be flagged:"
  local planted
  for planted in planted_html.svelte planted_inner.ts planted_outer.ts planted_adjacent.ts \
    planted_unsafe.ts planted_fragment.ts planted_write.ts planted_srcdoc.svelte \
    planted_function.ts planted_eval.ts planted_mermaid.ts planted_bind.svelte \
    planted_bracket.ts planted_assign.ts planted_parser.ts planted_spaced.svelte; do
    rc=0
    grep -q "$planted" <<<"$output" || rc=1
    note "$planted" "$rc"
  done

  echo "Files that must NOT be flagged:"
  for planted in innocent_mermaid.ts innocent_reader.svelte innocent_three_args.ts \
    innocent_textcontent.svelte BlockView.test.ts; do
    rc=0
    grep -q "$planted" <<<"$output" && rc=1
    note "$planted" "$rc"
  done

  rc=0
  [ "$status" -eq 1 ] || rc=1
  note "a planted sink fails the check (exit 1)" "$rc"

  # A check that CANNOT RUN must not report success — the failure mode that reached
  # production in the secret scanner once already. Outside any repository, so git fails.
  local outside
  outside="$(mktemp -d)"
  cd "$outside"
  set +e
  scan_only >/dev/null 2>&1
  status=$?
  set -e
  cd "$original"
  rm -rf "$outside"
  rc=0
  [ "$status" -eq 2 ] || rc=1
  note "a check that cannot run fails (exit 2) rather than passing" "$rc"

  echo
  if [ "$failures" -gt 0 ]; then
    echo "self-test: $failures failure(s)"
    return 1
  fi
  echo "self-test: every sink shape is caught and no innocent one is"
}

# A clean report is only trustworthy if the check could still have failed, so proving
# that is part of producing it rather than a separate step somebody can drop.
scan() {
  cd "$(git rev-parse --show-toplevel)"
  if ! self_test >/dev/null 2>&1; then
    echo "::error::the HTML-sink check failed its own self-test — its verdict means nothing" >&2
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
