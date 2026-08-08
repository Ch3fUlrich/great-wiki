#!/usr/bin/env python3
"""Check the bundled typefaces still hold the promises made in fonts.css.

    python3 web/scripts/check-fonts.py          # needs `pip install fonttools[woff]`

Exit code 0 = every check passed, 1 = at least one failed. Nothing is printed twice
and nothing is written, so it is safe to run anywhere, including in CI.

There is a smaller version of the central check that fits on one line, if all you want
is a smoke test without this file:

    python3 -c "from fontTools.ttLib import TTFont;import sys;assert 0x1E9E in TTFont(sys.argv[1]).getBestCmap()" <file>

This script exists because that one-liner is not enough. It checks four things:

1. U+1E9E (ẞ, capital sharp s) is in the cmap. On a German platform a missing ẞ turns
   STRAẞE into a box.

2. The ẞ is REAL. Several fonts satisfy the one-liner with a fake: a composite glyph
   built from two copies of S at roughly twice the advance width, so the character is
   "present" and renders as STRASSE. A genuine ẞ is drawn, and is only slightly wider
   than S — never near 2x. Both the composite structure and the advance ratio are
   checked here.

3. Every binary sits beside an OFL.txt, and still carries its copyright (name ID 0) and
   licence (name ID 13) internally. OFL 1.1 §2 requires the notice to travel with the
   font; a subsetting run with pyftsubset's default --name-IDs silently drops ID 13.

4. The files whose licence declares a Reserved Font Name are byte-for-byte upstream.
   Subsetting is Modification under OFL 1.1 §1, and Modification forfeits the reserved
   name — so for those families "unchanged" is a licence condition, not a preference,
   and a hash is the only way to state it that a future edit cannot talk its way past.
   A legitimate upstream version bump will fail here: that is the moment to re-read the
   new OFL.txt and update the hash deliberately.
"""

import hashlib
import sys
from pathlib import Path

try:
    from fontTools.ttLib import TTFont
except ImportError:  # pragma: no cover - depends on the environment, not on us
    sys.exit("fontTools is not installed: pip install 'fonttools[woff]'")

FONTS = Path(__file__).resolve().parent.parent / "static" / "fonts"

# Families whose OFL declares a Reserved Font Name must not be modified at all.
# sha256 of the upstream file, as shipped by the foundry.
UNMODIFIED = {
    "ibm-plex-sans/IBMPlexSansVar-Roman.woff2": (
        "18d275659b887e786cbea99db12c0ecb137699fd6ce848f5bff0c21f6257d8ea"
    ),
    "ibm-plex-sans/IBMPlexSansVar-Italic.woff2": (
        "9c88092a89b6ad070c40479ab4b3fac36959cb68e4484abed8883bdbaa14ff33"
    ),
    "ibm-plex-mono/IBMPlexMono-Regular.woff2": (
        "49ce58b41a0e1cb921c0f58d9a5b8b96a2cc21437c7066f3ba4f24873076d131"
    ),
}

# ä ö ü ß Ä Ö Ü ẞ, the euro, and the German quotation marks „ “ ‚ ‘.
GERMAN = "äöüßÄÖÜẞ€„“‚‘"

# A real ẞ is a little wider than S in a proportional face and exactly as wide in a
# monospaced one. Twice as wide means it is two S glued together.
MAX_ESZETT_TO_S_RATIO = 1.7

failures: list[str] = []


def fail(path: Path, message: str) -> None:
    failures.append(f"{path.relative_to(FONTS)}: {message}")


def check(path: Path) -> None:
    font = TTFont(path, lazy=True)
    cmap = font.getBestCmap()
    hmtx = font["hmtx"]

    missing = [c for c in GERMAN if ord(c) not in cmap]
    if missing:
        fail(path, f"missing German characters: {''.join(missing)}")

    if 0x1E9E not in cmap:
        fail(path, "no U+1E9E (ẞ) — STRAẞE would render as a box")
    elif 0x53 in cmap:
        eszett, s = cmap[0x1E9E], cmap[0x53]
        ratio = hmtx[eszett][0] / hmtx[s][0]
        if ratio > MAX_ESZETT_TO_S_RATIO:
            fail(path, f"U+1E9E is {ratio:.2f}x the width of S — that is a faked SS, not a ẞ")
        glyf = font.get("glyf")
        if glyf is not None and glyf[eszett].isComposite():
            parts = [c.glyphName for c in glyf[eszett].components]
            if all(p in ("S", "S.sc", "s") for p in parts):
                fail(path, f"U+1E9E is a composite of {parts} — a faked SS, not a ẞ")

    names = font["name"]
    for name_id, what in ((0, "copyright"), (13, "licence")):
        if not names.getDebugName(name_id):
            fail(path, f"name ID {name_id} ({what}) was stripped — OFL 1.1 §2 requires it")

    licence = path.parent / "OFL.txt"
    if not licence.is_file():
        fail(path, f"no OFL.txt beside it in {path.parent.name}/")

    key = f"{path.parent.name}/{path.name}"
    expected = UNMODIFIED.get(key)
    if expected is not None:
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            fail(
                path,
                "its licence reserves the family name, so it must be the untouched "
                f"upstream file — expected sha256 {expected}, found {actual}",
            )


def main() -> int:
    files = sorted(FONTS.glob("*/*.woff2"))
    if not files:
        sys.exit(f"no fonts found under {FONTS}")

    for path in files:
        check(path)

    # Every family directory must carry its licence, even one we ship no binary from.
    for directory in sorted(p for p in FONTS.iterdir() if p.is_dir()):
        if not (directory / "OFL.txt").is_file():
            failures.append(f"{directory.name}/: no OFL.txt")

    if failures:
        print(f"{len(failures)} font problem(s):", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1

    print(f"{len(files)} fonts checked, all sound (ẞ real, licences intact)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
