-- The event bus (ADR 0025): one row per (event, recipient), recording who MIGHT hear of
-- something — never who is entitled to.
--
-- **Ids and a kind, no words.** The row names the page by `doc_id`, the person by `actor`
-- and the comment or task by `subject` (opaque here: it is not a foreign key because it
-- points at different tables by `kind`). No title, no path of a page, no excerpt is stored,
-- so there is no second copy of a page's words for a grant change to leave behind. Title and
-- path are resolved at read time through the permission-checked accessor, for the reader.
--
-- `path` is the one exception and it is narrow: an admin event about an address that has no
-- page (an invitation into a space that is only a grant path). It is read only after the
-- reader is shown to still administer that path.
--
-- **Cascades.** A deleted recipient or page takes its rows with it; a deleted actor only
-- loses the name (SET NULL), because the event happened whoever did it.
--
-- **Dedupe.** UNIQUE(recipient, dedupe_key) where a key is given, so "edited by someone
-- else" coalesces into the recipient's one row per page rather than one row per keystroke
-- burst. Rows without a key are never coalesced.
CREATE TABLE events (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    recipient   TEXT NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    actor       TEXT REFERENCES principals(id) ON DELETE SET NULL,
    doc_id      TEXT REFERENCES documents(id) ON DELETE CASCADE,
    path        TEXT,
    subject     TEXT,
    dedupe_key  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    read_at     TEXT,
    digested_at TEXT
);

CREATE UNIQUE INDEX events_dedupe ON events (recipient, dedupe_key)
    WHERE dedupe_key IS NOT NULL;

CREATE INDEX events_recipient_created ON events (recipient, created_at);
