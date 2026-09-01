# 0014 — What a file has to be to be attached

**Status:** Accepted (2026-09-01)

## Context

D-16 makes what an attachment *is* a fact read from the bytes: `gw_store::blobs::sniff`
matches a closed allowlist of magic numbers, nothing looks at a declared `Content-Type` or a
filename extension, and unknown means refused. The thing being protected is the browser that
will later be handed those bytes with that type on them.

That rule had a cost, and
[`changelog.d/attachments.md`](../../changelog.d/attachments.md) stated it plainly: **plain
text, Markdown, CSV and SVG could not be attached at all.** None of them has anything in its
bytes that identifies it, so a closed signature allowlist cannot see them. For a wiki whose
corpus is prose and lab results, "you may attach a scan but not the CSV it came from" is a
real gap rather than a nuance, and the owner asked for all four.

The obvious move — add a signature for them — is not available, because there is nothing to
add. So the question is what *kind* of test replaces it, and what the answer is allowed to
claim.

## Decision

### A signature and a validity check are different questions, and both are asked

A signature is a **statement a format makes about itself**, found in a bounded prefix. It stays
exactly as it was: the closed allowlist, matched against the head, first.

Text makes no statement anywhere, so the second question is not "does this begin like text"
but **"is all of this text"** — every byte valid UTF-8, and no control character other than
tab, newline and carriage return. That is a validity check over the whole stream, and
`BlobWriter` folds it through the chunks as they arrive rather than deciding it from the head.
The difference matters: a file whose first kilobyte is a licence header and whose remainder is
a binary payload is not text, and only a check that sees all of it can say so.

**The signature is asked first.** A format that claims something is taken at its word, so
`%PDF-` is a PDF even though those five bytes are also perfectly good text. Only what claims
nothing falls through to the weaker, more general answer.

### Plain text, Markdown and CSV are all `text/plain; charset=utf-8`, and the wiki says so

Nothing in the bytes tells the three apart. A `.csv` and a `.md` differ by convention and by
what a reader does with them, not by anything measurable here, and sniffing for commas and
calling it CSV would be a guess dressed as a measurement — the same mistake as trusting a
declared type, arrived at from the other direction. The filename the page carries is where
"this is a spreadsheet" is written down, and that is a fact about the *attachment*, not about
the bytes.

The charset is not decoration. It is the one thing about these bytes that has been *proved*
rather than assumed, and stating it is what stops a browser guessing an encoding under which
the same bytes say something else.

**A byte order mark is accepted and never removed.** U+FEFF is valid UTF-8 and is not a
control character, so it needs no special case to pass — and it must not be stripped, because
the digest **is** the address (D-16). A store that altered bytes on the way in would hand back
a file whose hash the uploader cannot reproduce from their own copy, and would break the
property that the file at `blobs/<sha>` hashes to `<sha>`. UTF-16 carries a mark too, is not
valid UTF-8, and is refused rather than stored as text nobody could read.

### Bytes that are really something else, and are text, are still `text/plain`

An HTML page, a shell script, a JSON document: all accepted, all served as `text/plain`. This
is deliberate and it is not a hole.

Refusing HTML would refuse Markdown, because Markdown may legitimately contain HTML — there is
no line between them to draw. And being wrong costs nothing, because the wiki never *calls* it
HTML: the response says `text/plain`, `X-Content-Type-Options: nosniff` forbids the browser
from deciding otherwise, and `Content-Disposition: attachment` saves it rather than rendering
it. The dangerous version of this mistake is serving markup *as* markup, and nothing here can
produce that.

Bytes that are really a *binary* format cannot reach this branch: anything with a known
signature is typed by it, and anything without one that is nonetheless valid UTF-8 free of
control characters is, by definition, indistinguishable from text. Calling it text is the
safest true statement available about bytes nobody can identify.

### SVG is accepted, is never sanitised, and is never served inline

SVG is XML that can carry `<script>`, event handlers and external references: the one image
format that is also a program. All three clauses follow from that.

**Accepted**, and typed `image/svg+xml` rather than left as text, so that nothing downstream
has to guess what it is. Recognised by a deliberately shallow look at the head —
`looks_like_svg` skips a BOM and whitespace and asks whether the document opens with `<svg`,
or opens with an XML declaration and mentions `<svg` within the first kilobyte. It is not an
XML parser and must not become one; `gw_store::blobs`'s header says why no parser runs on this
path. Being wrong either way is cheap: an SVG it does not recognise is `text/plain`, stored and
served exactly as safely, and a text file it mistakes for an SVG is served under a disposition
a real one cannot escape either.

**Never sanitised.** Stripping script out of XML is a losing game — entity encodings, foreign
namespaces, `xlink:href`, CSS — and a half-sanitised file is worse than an honestly quarantined
one, because it invites being trusted. It is stored exactly as it arrived and never executed.

**Never served inline**, and that is checked *before* the rule that renders images rather than
carved out of it. `content_disposition` is a match whose first arm names `image/svg+xml`, so
the image branch is not reached, because a defence that depends on somebody remembering that
SVG is an image would not survive the next type being added. Two more headers back it up and
`an_svg_is_never_offered_inline_however_much_of_an_image_it_is` asserts all of them on an SVG
specifically rather than assuming header logic written for other types covers this one:
`nosniff`, and `Content-Security-Policy: default-src 'none'; sandbox`, which blocks script in
an opaque origin even if something did render it.

**The constraint this leaves for whoever renders an attachment in the interface**, stated here
and on `content_disposition` so it is met wherever it is looked for: an SVG may be shown
through `<img>` or a CSS `background-image` — contexts no browser executes script in — and
never through `<object>`, `<embed>`, `<iframe>`, or by putting its markup into this wiki's own
DOM. The last of those would execute it *in this origin*, with the session cookie in reach.
`Content-Disposition: attachment` also makes an `<iframe>` pointing at the download URL save
rather than render, so the disposition is already doing work for a page nobody has written yet.

## Consequences

- **What is refused is now a smaller and stranger set**, and the 415 message says so: a file
  that is neither a format the allowlist knows nor text at all. A WAV, an object file, a UTF-16
  document, a file that stops mid-character.
- **`sniff` is no longer the whole of the typing**, so a reader of `blobs.rs` who stops at the
  allowlist now has half the answer. `BlobWriter::media_type` is the entry point, and the
  module header leads with the two-questions distinction for that reason.
- **A text upload costs a scan of every byte** — UTF-8 validation and a control-character
  check, over 250 MB at worst. It runs on bytes already being hashed and written, so it is one
  more pass over data in cache rather than a second read.
- **Text is saved rather than rendered**, like everything that is not a picture or a PDF. A
  `.txt` opens a download dialogue. That is the closed answer and it is what `nosniff` plus
  `attachment` buys; making text inline would be a separate decision about a type that can be
  markup.

## Switch-back criteria

Revisit if any of these becomes true:

- **Somebody needs a `.csv` to be `text/csv` on the wire** — a client that dispatches on the
  media type, an export that feeds a spreadsheet. The answer would not be to sniff for commas;
  it would be to carry the uploaded filename's extension as a fact about the *attachment* and
  let the download derive a type from it, which is a change to what `attachments` records
  rather than to what `blobs` measures. Nothing may make that value influence what is stored.
- **An SVG needs to render in the page.** `<img src>` on the existing address already works
  and is safe; it does not need this decision changed. What would need it is wanting the markup
  in this wiki's DOM, and the answer to that is a rasterised copy, not a sanitiser.
- **A textual format arrives that must not be treated as text** — one where being handed
  `text/plain` with `nosniff` is itself a problem. None is known; the assumption is that the
  browser cannot be harmed by bytes it is told are text and will not render.
