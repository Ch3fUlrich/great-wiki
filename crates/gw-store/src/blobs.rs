//! The bytes: a content-addressed store on the media mount, and the only thing in this
//! workspace that decides what a file *is*.
//!
//! # Why this is not in the database
//!
//! AGENTS.md rule 5, and it is an architecture rule rather than a preference: the database,
//! the search index and the vectors live on NVMe; blobs live on `/mnt/cloud/great-wiki/media/`
//! — NFS. Never the other way round. So this module owns a directory and knows nothing about
//! permissions, and [`crate::attachments`] owns the rows and knows nothing about bytes.
//!
//! # Content-addressed, which is D-16 and not an optimisation
//!
//! A blob's name is the SHA-256 of its contents, so the same PDF attached to two pages is
//! one file. That is the storage half of D-16. The *access* half is that a download is
//! authorised against the page it was reached through, never against the blob — which is
//! only true as long as **a digest is never an address a reader can hold**. Nothing here
//! resolves a request to a blob; [`crate::attachments`] resolves a page and a filename to a
//! digest, after asking the permission engine, and hands it here.
//!
//! # Typed by the bytes, in two ways that are not the same kind of question
//!
//! `docs/decisions/0014-what-a-file-has-to-be-to-be-attached.md` is the decision; what
//! follows is the shape of it, where the code is.
//!
//! There is no parameter for a declared `Content-Type` and no code path that looks at a
//! filename extension, because a type that came from the request is a type the uploader
//! chose, and the thing being protected is the browser that will later be handed these bytes
//! with that type on them. What is left is the bytes, and they answer in two different ways.
//!
//! **A signature is a statement a format makes about itself.** [`sniff`] reads the leading
//! bytes and matches them against a closed allowlist: a PNG says PNG in eight of them. It
//! looks at a bounded prefix because that is where the statement is, and a format that makes
//! no such statement is invisible to it.
//!
//! **Text makes no statement anywhere, so the question has to be a different one.** Not
//! "does this begin like text" but "is ALL of this text" — every byte valid UTF-8, and no
//! control character other than tab, newline and carriage return. That is a *validity check*
//! and not a signature, and the difference is load-bearing: it is a property of the whole
//! stream, so [`BlobWriter`] decides it as the chunks go past rather than from the head. A
//! file whose first kilobyte is a licence header and whose remainder is a binary payload is
//! not text, and only a check that sees all of it can say so.
//!
//! Three consequences, each deliberate:
//!
//! * **`text/plain` is what plain text, Markdown and CSV are all served as**, because
//!   nothing in the bytes tells them apart. A `.csv` and a `.md` differ by convention and by
//!   what a reader does with them, not by anything measurable here, and sniffing for commas
//!   would be a guess dressed as a measurement. The filename the page carries is where
//!   "this is a spreadsheet" is written down, and that is a fact about the attachment rather
//!   than about the bytes.
//! * **A byte order mark is accepted and never removed.** U+FEFF is valid UTF-8 and is not a
//!   control character, so it needs no special case to pass — and it must not be stripped,
//!   because the digest IS the address (D-16): a store that altered the bytes on the way in
//!   would hand back a file whose hash the uploader cannot reproduce from their own copy.
//!   UTF-16, which also carries a mark, is not valid UTF-8 and is refused rather than stored
//!   as text nobody could read.
//! * **Bytes that are really some other textual format are `text/plain` too** — an HTML
//!   page, a shell script, a JSON document. This is not a hole. Markdown may legitimately
//!   contain HTML, so refusing HTML would refuse Markdown; and being wrong costs nothing,
//!   because the wiki never *calls* it HTML. `gw_api::routes::attachments` sends
//!   `text/plain`, `X-Content-Type-Options: nosniff` so the browser may not decide otherwise,
//!   and `Content-Disposition: attachment` so it is saved rather than rendered. Bytes that
//!   are really a *binary* format cannot get here: anything with a known signature is typed
//!   by it, and anything without one that is nonetheless text is, by definition,
//!   indistinguishable from text.
//!
//! **Unknown still means refused** (AGENTS.md rule 3). What is refused is now what is
//! neither: a WAV, a UTF-16 document, an object file, a file that stops mid-character.
//!
//! # SVG is the one image format that is also a program
//!
//! An SVG is XML that can carry `<script>`, event handlers and external references, and it
//! is accepted here — stored exactly as it arrived. **Nothing sanitises it**, deliberately:
//! stripping script out of XML is a losing game, and a half-sanitised file is worse than an
//! honestly quarantined one, because it invites being trusted. What makes it safe is
//! entirely on the way out, and lives in `gw_api::routes::attachments::content_disposition`:
//! an SVG is **never** served inline, whatever else is, and the `nosniff` and
//! `default-src 'none'; sandbox` headers back that up. The constraint for anything that ever
//! renders an attachment in the interface: an SVG may be shown through `<img>` or a CSS
//! background — contexts in which no browser executes it — and never through `<object>`,
//! `<embed>`, `<iframe>`, or by putting its markup into this wiki's own DOM.
//!
//! # No parser runs here
//!
//! [`sniff`] compares byte prefixes. It does not parse a PDF, walk a ZIP central directory
//! or read an ISO-BMFF box tree, and it must not learn to: text extraction from PDF and
//! Office files is a known attack surface and belongs in a background job. The consequence
//! is that an OOXML document (`.docx`) is `application/zip`, because that is what its first
//! four bytes honestly say and telling the two apart needs a parser.
//!
//! [`looks_like_svg`] is held to the same rule and is the closest thing here to an
//! exception, so its limits are written down where it is defined: it looks for a root
//! element inside a bounded prefix and it resolves nothing, validates nothing and expands no
//! entity. Everything it cannot answer within [`HEAD_BYTES`] is `text/plain`, which is the
//! closed answer and costs nothing.
//!
//! # Why [`BlobStore`] is a struct and not a trait
//!
//! Both the plan and `README.md` say "content-addressed blob store **behind a swappable
//! trait**", and it is not one. The swap the trait exists for is the one AGENTS.md rule 5
//! forbids: blobs live on `/mnt/cloud/great-wiki/media/`, and "never a database or an object
//! store on NFS" rules out the S3-shaped second implementation a `BlobStore` trait is always
//! written for. What is left is one implementation, and a seam with one thing on each side is
//! a seam nothing keeps honest.
//!
//! It buys nothing in the tests either, which is usually the other half of the argument: the
//! interesting failure is "the row survived and the file did not", and a test proves that by
//! removing the file — exercising the real code — rather than by substituting a fake that
//! reports what it was told to. `a_file_whose_bytes_have_gone_answers_503_and_not_404` does
//! exactly that.
//!
//! Add the trait when there is a second implementation, and not before. The shape here does
//! not resist one: every caller already goes through this type's four methods.

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// D-17: 250 MB per file.
///
/// Enough for scan bundles and short video; a single accidental upload is noticeable but not
/// damaging. Enforced twice on purpose — `gw_api::routes::attachments` puts a
/// `RequestBodyLimitLayer` of exactly this size on the upload route so a body that declares
/// itself larger is refused before a byte is read, and [`BlobWriter`] counts what actually
/// arrives so a chunked upload that declares nothing is refused as it goes.
pub const MAX_ATTACHMENT_BYTES: u64 = 250 * 1024 * 1024;

/// How many leading bytes are kept for typing.
///
/// A signature needs twelve at most. The rest is for [`looks_like_svg`], which has to see
/// past an XML declaration, a comment and a DOCTYPE to find a root element — and which stops
/// looking here rather than following a comment of arbitrary length. A kilobyte per upload
/// in flight is nothing beside the 250 MB the upload itself may be.
const HEAD_BYTES: usize = 1024;

/// What text is served as, charset included.
///
/// The charset is not decoration: it is the one thing about these bytes that has been
/// *proved* rather than assumed — [`TextScan`] validated every one of them as UTF-8 — and
/// stating it is what stops a browser guessing an encoding under which the same bytes say
/// something else.
const TEXT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";

/// The media types this wiki will store, and the byte signatures that identify them.
///
/// A closed allowlist, in the order it is tried. Anything not matched here is refused —
/// see the module header for why "guess `text/plain`" is not the alternative it looks like.
///
/// `None` in the second position of a pair means "any byte here", which is what
/// `RIFF????WEBP` and ISO-BMFF's `????ftyp` need.
type Signature = (&'static [(usize, u8)], &'static str);

/// Take the leading bytes of a file and match them against the signature allowlist.
///
/// `None` means "no format claimed these bytes", including for an empty prefix and one too
/// short to carry a signature. It is never "probably text" — text is not a signature, and
/// [`TextScan`] answers that question over the whole stream instead.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    /// `(offset, byte)` pairs that must all match.
    const SIGNATURES: &[Signature] = &[
        // PNG's signature is eight bytes and includes a CRLF pair specifically so that a
        // transfer which mangles line endings corrupts the header rather than the image.
        (
            &[
                (0, 0x89),
                (1, b'P'),
                (2, b'N'),
                (3, b'G'),
                (4, 0x0D),
                (5, 0x0A),
                (6, 0x1A),
                (7, 0x0A),
            ],
            "image/png",
        ),
        (&[(0, 0xFF), (1, 0xD8), (2, 0xFF)], "image/jpeg"),
        (
            &[(0, b'G'), (1, b'I'), (2, b'F'), (3, b'8'), (5, b'a')],
            "image/gif",
        ),
        // RIFF container, WEBP payload. Bytes 4..8 are the length and are deliberately not
        // constrained.
        (
            &[
                (0, b'R'),
                (1, b'I'),
                (2, b'F'),
                (3, b'F'),
                (8, b'W'),
                (9, b'E'),
                (10, b'B'),
                (11, b'P'),
            ],
            "image/webp",
        ),
        (&[(0, b'I'), (1, b'I'), (2, 0x2A), (3, 0x00)], "image/tiff"),
        (&[(0, b'M'), (1, b'M'), (2, 0x00), (3, 0x2A)], "image/tiff"),
        (
            &[(0, b'%'), (1, b'P'), (2, b'D'), (3, b'F'), (4, b'-')],
            "application/pdf",
        ),
        // ISO base media format: a `ftyp` box at offset 4. Covers MP4 and the MOV/M4V
        // family, which this serves as `video/mp4` rather than parsing the brand — reading
        // the brand is the beginning of a parser, and the module header says why there is
        // not one.
        (&[(4, b'f'), (5, b't'), (6, b'y'), (7, b'p')], "video/mp4"),
        // Matroska/WebM's EBML header. Telling the two apart needs the DocType element,
        // which is a parse; `video/webm` is what a browser needs for either.
        (&[(0, 0x1A), (1, 0x45), (2, 0xDF), (3, 0xA3)], "video/webm"),
        (&[(0, b'O'), (1, b'g'), (2, b'g'), (3, b'S')], "audio/ogg"),
        // ZIP, in all three of its local-header spellings. An OOXML document, an ODF
        // document and an EPUB are all this, honestly: see the module header.
        (
            &[(0, b'P'), (1, b'K'), (2, 0x03), (3, 0x04)],
            "application/zip",
        ),
        (
            &[(0, b'P'), (1, b'K'), (2, 0x05), (3, 0x06)],
            "application/zip",
        ),
        (
            &[(0, b'P'), (1, b'K'), (2, 0x07), (3, 0x08)],
            "application/zip",
        ),
    ];

    SIGNATURES
        .iter()
        .find(|(pattern, _)| {
            pattern
                .iter()
                .all(|(offset, byte)| head.get(*offset) == Some(byte))
        })
        .map(|(_, media_type)| *media_type)
}

/// Whether these leading bytes open an SVG document.
///
/// Deliberately shallow, and the shallowness is the point — the module header says no parser
/// runs here. It skips a byte order mark and leading whitespace, and then asks one of two
/// questions:
///
/// * does the document open with `<svg`, which is what an exported icon does; or
/// * does it open with an XML declaration and mention `<svg` within [`HEAD_BYTES`], which is
///   what an editor writes — a declaration, then a comment or a DOCTYPE, then the root.
///
/// It resolves no entity, reads no attribute and validates nothing. Being wrong in either
/// direction is cheap and that is why so little machinery is justified: an SVG this does not
/// recognise is `text/plain`, stored and served exactly as safely; and a text file this
/// mistakes for an SVG is served as `image/svg+xml` under the same
/// `Content-Disposition: attachment` a real one cannot escape either.
fn looks_like_svg(head: &[u8]) -> bool {
    let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let start = head
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(head.len());
    let head = &head[start..];
    if head.starts_with(b"<svg") {
        return true;
    }
    head.starts_with(b"<?xml") && head.windows(4).any(|window| window == b"<svg")
}

/// The running answer to "is all of this text", kept across the chunks of one upload.
///
/// A signature is decided from the head; this cannot be, because the property is about every
/// byte. So it is folded through the stream: each chunk is validated as UTF-8 and scanned
/// for control characters, and the verdict can only ever go from true to false.
///
/// The awkward part is that a multi-byte character does not respect a chunk boundary. An
/// incomplete sequence at the end of a chunk is ordinary and is carried into the next one;
/// an incomplete sequence at the end of the FILE is a truncated file and is not text. The
/// two look identical byte-for-byte, and [`TextScan::partial`] is what tells them apart.
#[derive(Debug)]
struct TextScan {
    /// False once anything has disqualified the stream. Never returns to true.
    textual: bool,
    /// The tail of a character split across a chunk boundary — at most three bytes.
    /// Non-empty when the upload ends means the file stops mid-character.
    partial: Vec<u8>,
}

impl TextScan {
    fn new() -> Self {
        Self {
            textual: true,
            partial: Vec::new(),
        }
    }

    /// Fold one chunk into the verdict.
    fn push(&mut self, chunk: &[u8]) {
        if !self.textual {
            return;
        }
        let joined: std::borrow::Cow<'_, [u8]> = if self.partial.is_empty() {
            std::borrow::Cow::Borrowed(chunk)
        } else {
            let mut joined = std::mem::take(&mut self.partial);
            joined.extend_from_slice(chunk);
            std::borrow::Cow::Owned(joined)
        };
        let bytes = joined.as_ref();

        let (valid, tail) = match std::str::from_utf8(bytes) {
            Ok(text) => (text, &bytes[bytes.len()..]),
            // `error_len() == None` is the one recoverable failure: the input ran out
            // part-way through a character, which is what a chunk boundary looks like.
            // Anything else is a byte sequence that cannot be UTF-8 at all.
            Err(error) if error.error_len().is_none() => {
                let split = error.valid_up_to();
                let valid = std::str::from_utf8(&bytes[..split])
                    .expect("`valid_up_to` is by definition valid");
                (valid, &bytes[split..])
            }
            Err(_) => {
                self.textual = false;
                return;
            }
        };

        // Tab, newline and carriage return are how text is laid out; every other control
        // character — a NUL, a bell, a C1 escape — is how a binary file that happens to
        // decode gives itself away. `char::is_control` covers C0, DEL and C1 alike.
        if valid
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
        {
            self.textual = false;
            return;
        }

        self.partial.clear();
        self.partial.extend_from_slice(tail);
    }

    /// What the finished stream is, or `None` if it was never text.
    ///
    /// `head` is only consulted to tell an SVG from everything else; the *acceptance* was
    /// decided by the fold above, over all of it.
    fn media_type(&self, head: &[u8]) -> Option<&'static str> {
        if !self.textual || !self.partial.is_empty() {
            return None;
        }
        Some(if looks_like_svg(head) {
            "image/svg+xml"
        } else {
            TEXT_MEDIA_TYPE
        })
    }
}

/// Bytes that are on the mount under their own digest, and everything the database records
/// about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// Lowercase hex SHA-256. **Never put in a URL** — see the module header.
    pub sha256: String,
    /// What arrived, counted while hashing it. Never a `Content-Length` a client asserted.
    pub byte_size: i64,
    /// What [`sniff`] made of the leading bytes.
    pub media_type: &'static str,
}

/// Bytes that have arrived and been typed, and are **not yet in the store**.
///
/// The gap between this and [`StoredBlob`] is where the permission check goes, and that is
/// the whole reason the type exists. An upload has to be read before anything about it is
/// known — its digest is a function of all of it — so the alternative shapes are: publish the
/// bytes and then ask whether the caller may attach them, which puts a stranger's file on the
/// mount under a name that will never be referenced; or ask first and read second, which is
/// an authorisation decision taken somewhere other than the operation it authorises, and this
/// project's rule is that the second such decision is the one that gets it wrong.
///
/// So the bytes wait in `tmp/`, [`crate::Store::attach`] decides, and only a `Done` publishes
/// them. Dropping one instead — a refusal, an error, a client that hung up — removes the
/// temporary file.
#[derive(Debug)]
pub struct PendingBlob {
    root: PathBuf,
    /// `None` once published or removed.
    temp: Option<PathBuf>,
    sha256: String,
    byte_size: i64,
    media_type: &'static str,
}

impl PendingBlob {
    /// What these bytes are, before anybody has decided whether to keep them.
    ///
    /// Enough to record a refusal or write a log line; not enough to reach the file, because
    /// there is not one yet.
    pub fn describe(&self) -> StoredBlob {
        StoredBlob {
            sha256: self.sha256.clone(),
            byte_size: self.byte_size,
            media_type: self.media_type,
        }
    }

    /// Move the bytes into the store, under their digest.
    ///
    /// **The rename happens even when the digest is already there**, deliberately, and it
    /// does two things. It removes the timing difference between storing novel bytes and
    /// storing bytes somebody else already had — see [`crate::attachments`] for why an upload
    /// must disclose nothing about what is already stored. And it repairs a file that went
    /// missing while its row survived: re-uploading is the fix for a stale mount, and it
    /// would not be one if the write were skipped whenever the digest was already known.
    pub fn publish(mut self) -> impl std::future::Future<Output = Result<StoredBlob>> + Send {
        let described = self.describe();
        let destination = blob_path(&self.root, &self.sha256);
        let temp = self.temp.take();
        async move {
            let destination = destination?;
            let temp = temp.expect("a pending blob is published exactly once");
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&temp, &destination).await?;
            Ok(described)
        }
    }
}

impl Drop for PendingBlob {
    fn drop(&mut self) {
        remove_abandoned(self.temp.take());
    }
}

/// Remove a temporary file nothing is going to publish.
///
/// Best effort and synchronous, because [`Drop`] cannot await. Named in the log so a mount
/// filling up has something to explain it. Shared by [`PendingBlob`] and [`BlobWriter`], so
/// an upload abandoned at either stage is cleaned up the same way.
fn remove_abandoned(temp: Option<PathBuf>) {
    let Some(temp) = temp else { return };
    if temp.exists() {
        if let Err(error) = std::fs::remove_file(&temp) {
            tracing::warn!(%error, ?temp, "could not remove an abandoned upload");
        }
    }
}

/// What an upload came to.
///
/// Four outcomes rather than an error, because three of them are the caller's mistake and
/// each has a different fix — the shape [`crate::TrashOutcome`] and
/// [`crate::MembershipOutcome`] both take. A genuine failure (the mount is gone, the disk is
/// full) is still an `Err`, because there is nothing the caller can do about it.
#[derive(Debug)]
pub enum BlobOutcome {
    /// Read, hashed and typed, and waiting in `tmp/` for somebody to authorise it.
    Accepted(PendingBlob),
    /// Past [`MAX_ATTACHMENT_BYTES`] (D-17).
    TooLarge,
    /// Neither a signature nor text: the leading bytes match nothing in [`sniff`]'s
    /// allowlist, and the stream as a whole is not valid UTF-8 free of control characters.
    UnknownType,
    /// Nothing arrived. Refused rather than stored, because a zero-byte attachment is a
    /// failed upload that would otherwise look like a successful one in the `Anhänge` list.
    Empty,
}

/// [`BlobStore::path_for`], as a function of the root, so [`BlobWriter`] can reach it
/// without holding a store. One body, so the writer and the reader cannot come to disagree
/// about where a digest lives.
fn blob_path(root: &Path, sha256: &str) -> Result<PathBuf> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("`{sha256}` is not a lowercase hex SHA-256 and cannot name a stored file");
    }
    Ok(root
        .join("blobs")
        .join(&sha256[0..2])
        .join(&sha256[2..4])
        .join(sha256))
}

/// A directory of content-addressed files.
///
/// Two subdirectories, and the second one is load-bearing rather than tidy: `blobs/` holds
/// finished files and `tmp/` holds ones still arriving. They are under the same root — and
/// therefore on the same filesystem — so the move from one to the other is a `rename`, which
/// is atomic. A partly-written file is never visible under its own digest, which is what
/// makes "the file at `blobs/…/<sha>` hashes to `<sha>`" true rather than intended.
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Open (creating if needed) the media directory.
    ///
    /// Fails rather than degrading if the directory cannot be made: AGENTS.md rule 3, and a
    /// server that starts with no usable media directory would accept an upload and lose it.
    /// `gw_api`'s `serve` therefore calls this before it binds a listener.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(root.join("blobs"))?;
        std::fs::create_dir_all(root.join("tmp"))?;
        Ok(Self { root })
    }

    /// The directory this store was opened on. For diagnostics; nothing resolves a path
    /// through it.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the bytes with this digest live.
    ///
    /// Two levels of fan-out on the first four hex characters, so a corpus of tens of
    /// thousands of files is not one directory with tens of thousands of entries in it.
    ///
    /// **It refuses a digest that is not one.** The path is built by concatenation, so a
    /// value holding `..` or `/` would be a traversal — and while nothing today can produce
    /// one (the digest is computed here, and the column carrying it is CHECKed to 64 lowercase
    /// hex characters by `0013_attachments.sql`), the defence that survives a second writer
    /// being added is the one at the point of use.
    pub fn path_for(&self, sha256: &str) -> Result<PathBuf> {
        blob_path(&self.root, sha256)
    }

    /// Open the bytes with this digest for reading, or `None` if they are not there.
    ///
    /// `None` covers both "never stored" and "the row survived but the file did not" — a
    /// stale NFS handle, a restore that missed the mount, a file removed by hand. The two are
    /// indistinguishable here and the caller must not treat either as "no such attachment":
    /// `gw_api::routes::attachments` answers 503 for both, because the wiki knows about the
    /// file and cannot serve it, which is not the same statement as 404.
    pub async fn open_read(&self, sha256: &str) -> Result<Option<tokio::fs::File>> {
        let path = self.path_for(sha256)?;
        match tokio::fs::File::open(&path).await {
            Ok(file) => Ok(Some(file)),
            // Every error is "cannot serve these bytes now", including `ESTALE` from an NFS
            // mount that the host still thinks is fine. Logged with the reason, because the
            // status code deliberately does not carry it.
            Err(error) => {
                tracing::warn!(%error, ?path, "stored bytes could not be opened");
                Ok(None)
            }
        }
    }

    /// Begin an upload, capped at [`MAX_ATTACHMENT_BYTES`] (D-17).
    ///
    /// This is the only spelling a server uses.
    pub fn writer(&self) -> Result<BlobWriter> {
        self.writer_with_limit(MAX_ATTACHMENT_BYTES)
    }

    /// [`BlobStore::writer`] with the cap as a parameter, so the refusal can be tested at a
    /// size a test can afford rather than by writing a quarter of a gigabyte into CI.
    ///
    /// It is not a second door into the store: the cap only ever narrows what is accepted,
    /// so the worst a caller can do with it is refuse more. `the_cap_is_d_17s_number` pins
    /// what [`BlobStore::writer`] passes.
    pub fn writer_with_limit(&self, max_bytes: u64) -> Result<BlobWriter> {
        let temp = self.root.join("tmp").join(uuid::Uuid::now_v7().to_string());
        Ok(BlobWriter {
            root: self.root.clone(),
            temp: Some(temp),
            file: None,
            hasher: Sha256::new(),
            head: Vec::with_capacity(HEAD_BYTES),
            text: TextScan::new(),
            byte_size: 0,
            max_bytes,
            over: false,
        })
    }
}

/// One upload in progress.
///
/// Streamed rather than buffered: D-17 allows 250 MB per file, and holding that in memory
/// per concurrent upload is how a wiki with four users falls over. The digest, the size and
/// the leading bytes are all accumulated as the chunks go past, so nothing is read twice and
/// the file is never in memory whole.
///
/// Dropping one — a client that disconnected, a handler that returned early — removes the
/// temporary file. Without that, every abandoned upload would leave a quarter-gigabyte on the
/// mount under a name nothing refers to, which is the invisible litter D-15 rejects the
/// inline-only design for, one layer down.
pub struct BlobWriter {
    root: PathBuf,
    /// `None` once the temporary file has been renamed into place or removed, so [`Drop`]
    /// knows there is nothing left to clean up.
    temp: Option<PathBuf>,
    file: Option<tokio::fs::File>,
    hasher: Sha256,
    head: Vec<u8>,
    /// The textual verdict, folded through the stream. See [`TextScan`] for why this cannot
    /// be decided from `head` the way a signature is.
    text: TextScan,
    byte_size: u64,
    max_bytes: u64,
    /// Set once the cap is passed. The write stops there; [`BlobWriter::finish`] is what
    /// reports it, so a caller draining a stream does not have to check every chunk.
    over: bool,
}

impl BlobWriter {
    /// The cap this writer will refuse past. Exposed so a test can prove
    /// [`BlobStore::writer`] passes D-17's number rather than some other one.
    pub fn limit(&self) -> u64 {
        self.max_bytes
    }

    /// What has arrived, or `None` for bytes this wiki will not store.
    ///
    /// **The signature is asked first, and that ordering is a decision.** A format that makes
    /// a statement about itself is taken at its word, so `%PDF-` is a PDF even though those
    /// five bytes are also perfectly good text. Only what claims nothing falls through to the
    /// textual check, which is the weaker, more general answer and belongs after the specific
    /// one.
    fn media_type(&self) -> Option<&'static str> {
        sniff(&self.head).or_else(|| self.text.media_type(&self.head))
    }

    /// Take the next chunk of the body.
    ///
    /// Once the cap is passed nothing more is written or hashed — the remaining bytes are
    /// counted and dropped. A caller may keep pushing (draining a request body is often
    /// simpler than aborting it) and will get [`BlobOutcome::TooLarge`] from
    /// [`BlobWriter::finish`] either way.
    pub async fn push(&mut self, chunk: &[u8]) -> Result<()> {
        self.byte_size = self.byte_size.saturating_add(chunk.len() as u64);
        if self.byte_size > self.max_bytes {
            self.over = true;
            return Ok(());
        }
        if self.head.len() < HEAD_BYTES {
            let want = HEAD_BYTES - self.head.len();
            self.head.extend_from_slice(&chunk[..want.min(chunk.len())]);
        }
        self.text.push(chunk);
        self.hasher.update(chunk);

        // Opened on the first chunk rather than in `writer()`, so a refused upload that never
        // sent a byte never touches the mount at all.
        if self.file.is_none() {
            let temp = self
                .temp
                .as_ref()
                .expect("a writer that has not finished still owns its temporary path");
            self.file = Some(tokio::fs::File::create(temp).await?);
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(chunk).await?;
        }
        Ok(())
    }

    /// Close the upload and type it.
    ///
    /// The order is the point. Size and type are decided before anything is offered to the
    /// rest of the system, so a refused upload leaves the store exactly as it found it — the
    /// temporary file is removed on the way out of every refusing branch, by
    /// [`PendingBlob`]'s and this writer's [`Drop`].
    ///
    /// What comes back on success is a [`PendingBlob`], **not** a stored one: the bytes are
    /// still in `tmp/` and go nowhere until somebody with the permission to attach them says
    /// so. See [`PendingBlob`] for why the check cannot be made before this point.
    pub async fn finish(mut self) -> Result<BlobOutcome> {
        if self.over {
            return Ok(BlobOutcome::TooLarge);
        }
        if self.byte_size == 0 {
            return Ok(BlobOutcome::Empty);
        }
        let Some(media_type) = self.media_type() else {
            return Ok(BlobOutcome::UnknownType);
        };

        // Durable before it is visible. On a mount that can go away mid-write, a file that
        // exists under a digest it does not hash to would be undetectable corruption; a
        // failure here is a refused upload, which is recoverable.
        if let Some(file) = self.file.as_mut() {
            file.sync_all().await?;
        }
        self.file = None;

        Ok(BlobOutcome::Accepted(PendingBlob {
            root: self.root.clone(),
            temp: self.temp.take(),
            sha256: format!("{:x}", self.hasher.finalize_reset()),
            byte_size: self.byte_size as i64,
            media_type,
        }))
    }
}

impl Drop for BlobWriter {
    fn drop(&mut self) {
        remove_abandoned(self.temp.take());
    }
}

#[cfg(test)]
mod tests {
    //! What the store does with bytes, what it refuses, and what it leaves behind.

    use super::{sniff, BlobOutcome, BlobStore, StoredBlob, MAX_ATTACHMENT_BYTES};

    fn png() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(b"the rest of a picture");
        bytes
    }

    /// Every file the store holds under `blobs/`, by digest.
    fn stored(store: &BlobStore) -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), out);
                } else {
                    out.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        let mut out = Vec::new();
        walk(&store.root().join("blobs"), &mut out);
        out.sort();
        out
    }

    fn temporaries(store: &BlobStore) -> usize {
        std::fs::read_dir(store.root().join("tmp")).unwrap().count()
    }

    /// Push, close, and publish — the whole of an accepted upload.
    async fn write(store: &BlobStore, bytes: &[u8]) -> Result<StoredBlob, BlobOutcome> {
        let mut writer = store.writer().unwrap();
        writer.push(bytes).await.unwrap();
        match writer.finish().await.unwrap() {
            BlobOutcome::Accepted(pending) => Ok(pending.publish().await.unwrap()),
            refusal => Err(refusal),
        }
    }

    /// The same, stopping where the permission check would be: accepted, but not published.
    async fn accept(store: &BlobStore, bytes: &[u8]) -> BlobOutcome {
        accept_chunks(store, &[bytes]).await
    }

    /// An upload that arrives in more than one piece, which is what every real one does.
    ///
    /// It matters for the textual half of the typing and for nothing else: a signature is
    /// decided from the head, so a chunk boundary cannot move it, while "all of this is
    /// text" is a property of every byte and therefore of the boundaries too.
    async fn accept_chunks(store: &BlobStore, chunks: &[&[u8]]) -> BlobOutcome {
        let mut writer = store.writer().unwrap();
        for chunk in chunks {
            writer.push(chunk).await.unwrap();
        }
        writer.finish().await.unwrap()
    }

    /// What an accepted upload was typed as, or `None` for a refusal.
    fn typed(outcome: BlobOutcome) -> Option<&'static str> {
        match outcome {
            BlobOutcome::Accepted(pending) => Some(pending.describe().media_type),
            _ => None,
        }
    }

    fn refusal(outcome: BlobOutcome) -> String {
        format!("{outcome:?}")
    }

    #[tokio::test]
    async fn bytes_are_named_by_their_own_digest() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        let blob = write(&store, &png()).await.expect("a PNG must be accepted");
        let expected = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(png()))
        };
        assert_eq!(blob.sha256, expected);
        assert_eq!(blob.media_type, "image/png");
        assert_eq!(blob.byte_size, png().len() as i64);
        assert_eq!(stored(&store), vec![expected.clone()]);

        // And the file really is at the fanned-out address, not merely somewhere.
        let path = store.path_for(&expected).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), png());
    }

    #[tokio::test]
    async fn the_same_bytes_stored_twice_are_one_file() {
        // D-16's storage half: one PDF on two pages is one copy.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        let first = write(&store, &png()).await.unwrap();
        let second = write(&store, &png()).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(stored(&store).len(), 1);
        assert_eq!(temporaries(&store), 0);
    }

    #[tokio::test]
    async fn bytes_nobody_publishes_never_reach_the_store() {
        // The window the permission check sits in: read, hashed, typed — and refused. This
        // is what stops an upload from a caller who may not attach anything landing on the
        // mount anyway.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        {
            let accepted = accept(&store, &png()).await;
            assert!(matches!(accepted, BlobOutcome::Accepted(_)));
            assert_eq!(temporaries(&store), 1, "the bytes wait in tmp/");
            assert!(stored(&store).is_empty(), "and are not in the store yet");
        }
        assert_eq!(
            temporaries(&store),
            0,
            "dropping the pending blob removes them"
        );
        assert!(stored(&store).is_empty());
    }

    #[tokio::test]
    async fn a_pending_blob_describes_itself_before_anybody_decides() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let BlobOutcome::Accepted(pending) = accept(&store, &png()).await else {
            panic!("a PNG must be accepted");
        };
        let described = pending.describe();
        assert_eq!(described.media_type, "image/png");
        assert_eq!(described.byte_size, png().len() as i64);
        assert_eq!(pending.publish().await.unwrap(), described);
    }

    #[tokio::test]
    async fn re_storing_bytes_whose_file_went_missing_puts_them_back() {
        // The recovery path for a stale mount: the row survives, the file does not, and
        // uploading the same file again is the fix. It only works because `publish` renames
        // even when the digest is already known — which is also what keeps the timing of a
        // deduplicated upload indistinguishable from a novel one.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let blob = write(&store, &png()).await.unwrap();
        std::fs::remove_file(store.path_for(&blob.sha256).unwrap()).unwrap();
        assert!(store.open_read(&blob.sha256).await.unwrap().is_none());

        write(&store, &png()).await.unwrap();
        assert!(store.open_read(&blob.sha256).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn bytes_that_are_not_there_are_none_rather_than_an_error() {
        // A missing file is the stale-mount case and is not a failure of this layer: the
        // caller decides what to say about it, and 404 is deliberately not the answer.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let absent = "0".repeat(64);
        assert!(store.open_read(&absent).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_type_the_allowlist_does_not_know_is_refused_and_nothing_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        // Bytes that are neither a signature nor text. Each one is a real format a wiki
        // might plausibly be handed, and each is refused because nothing here can say what
        // it is: a WAV (RIFF without WEBP), a UTF-16 document, a compiled object file.
        for bytes in [
            &b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00"[..],
            &b"\xff\xfeT\x00e\x00x\x00t\x00"[..],
            &b"\x7fELF\x02\x01\x01\x00"[..],
            // Valid UTF-8 up to the last byte, and then it stops mid-character.
            &b"beinahe Text \xc3"[..],
            // One control character that is not tab, newline or carriage return.
            &b"eine Zeile\x07mit einer Glocke"[..],
        ] {
            assert_eq!(
                refusal(accept(&store, bytes).await),
                refusal(BlobOutcome::UnknownType),
                "{bytes:?}"
            );
        }
        assert!(stored(&store).is_empty(), "nothing may reach the mount");
        assert_eq!(temporaries(&store), 0, "and nothing may be left behind");
    }

    #[tokio::test]
    async fn nothing_at_all_is_refused_rather_than_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert_eq!(
            refusal(accept(&store, b"").await),
            refusal(BlobOutcome::Empty)
        );
        assert!(stored(&store).is_empty());
        assert_eq!(temporaries(&store), 0);
    }

    #[tokio::test]
    async fn past_the_cap_is_refused_and_nothing_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        let mut writer = store.writer_with_limit(16).unwrap();
        writer.push(&png()).await.unwrap();
        writer.push(&png()).await.unwrap();
        assert_eq!(
            refusal(writer.finish().await.unwrap()),
            refusal(BlobOutcome::TooLarge)
        );
        assert!(stored(&store).is_empty());
        assert_eq!(temporaries(&store), 0);

        // Exactly at the cap is not past it.
        let mut writer = store.writer_with_limit(png().len() as u64).unwrap();
        writer.push(&png()).await.unwrap();
        assert!(matches!(
            writer.finish().await.unwrap(),
            BlobOutcome::Accepted(_)
        ));
    }

    #[test]
    fn the_cap_is_d_17s_number() {
        // The refusal above is tested at a size CI can afford, so this is what pins the one
        // a server actually uses. D-17: 250 MB per file.
        assert_eq!(MAX_ATTACHMENT_BYTES, 250 * 1024 * 1024);
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert_eq!(store.writer().unwrap().limit(), MAX_ATTACHMENT_BYTES);
    }

    #[tokio::test]
    async fn an_abandoned_upload_leaves_nothing_on_the_mount() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        {
            let mut writer = store.writer().unwrap();
            writer.push(&png()).await.unwrap();
            // Dropped without finishing: a client that hung up mid-upload.
        }
        assert_eq!(temporaries(&store), 0);
        assert!(stored(&store).is_empty());
    }

    #[test]
    fn a_digest_that_is_not_one_never_becomes_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        for bad in [
            "../../../etc/passwd",
            "..",
            "",
            "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789",
            "zz34567890123456789012345678901234567890123456789012345678901234",
        ] {
            assert!(
                store.path_for(bad).is_err(),
                "`{bad}` must not resolve to a path"
            );
        }
    }

    #[test]
    fn the_allowlist_types_by_the_bytes_and_not_by_anything_a_client_said() {
        for (bytes, expected) in [
            (&b"\x89PNG\r\n\x1a\nrest"[..], Some("image/png")),
            (&b"\xff\xd8\xff\xe0JFIF"[..], Some("image/jpeg")),
            (&b"GIF89a....."[..], Some("image/gif")),
            (&b"GIF87a....."[..], Some("image/gif")),
            (&b"RIFF\x10\x00\x00\x00WEBPVP8 "[..], Some("image/webp")),
            (&b"II\x2a\x00rest"[..], Some("image/tiff")),
            (&b"MM\x00\x2arest"[..], Some("image/tiff")),
            (&b"%PDF-1.7\nrest"[..], Some("application/pdf")),
            (&b"\x00\x00\x00\x20ftypisom"[..], Some("video/mp4")),
            (&b"\x1a\x45\xdf\xa3rest"[..], Some("video/webm")),
            (&b"OggS\x00\x02rest"[..], Some("audio/ogg")),
            (&b"PK\x03\x04rest"[..], Some("application/zip")),
            // The whole point, in one line: bytes that are a PNG are a PNG whatever the
            // upload claimed, and bytes that are HTML are refused however they were named.
            (&b"<!doctype html>"[..], None),
            (&b"text/plain has no magic number"[..], None),
            (&b""[..], None),
            (&b"\x89PN"[..], None),
            // RIFF without WEBP is a WAV or an AVI, neither of which is on the list.
            (&b"RIFF\x10\x00\x00\x00WAVEfmt "[..], None),
        ] {
            assert_eq!(sniff(bytes), expected, "{bytes:?}");
        }
    }

    // ---------------------------------------------------------------------------------
    // The textual half: a validity check rather than a signature.
    // ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn text_markdown_and_csv_are_all_one_type_because_the_bytes_do_not_say() {
        // The honest answer, asserted rather than described: nothing in these bytes tells
        // the three apart, so the wiki does not pretend it can. Sniffing for commas and
        // calling it CSV would be a guess dressed as a measurement.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        for bytes in [
            &b"nur ein paar Worte\n"[..],
            &b"# Ueberschrift\n\n- ein Punkt\n- noch einer\n"[..],
            &b"Name,Wert\nBlutdruck,120/80\n"[..],
            // A tab, a CRLF and a non-ASCII character are all ordinary text.
            "Spalte\tWert\r\nGr\u{f6}\u{df}e\t42\r\n".as_bytes(),
        ] {
            assert_eq!(
                typed(accept(&store, bytes).await),
                Some("text/plain; charset=utf-8"),
                "{bytes:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_svg_is_typed_as_the_image_it_is_and_not_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        for bytes in [
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>"[..],
            // What an editor writes: a declaration, a comment, then the root element.
            &b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- Inkscape -->\n<svg/>"[..],
            // A byte order mark and leading whitespace in front of it change nothing.
            &b"\xef\xbb\xbf\n  <svg viewBox=\"0 0 1 1\"/>"[..],
        ] {
            assert_eq!(
                typed(accept(&store, bytes).await),
                Some("image/svg+xml"),
                "{bytes:?}"
            );
        }

        // And an SVG carrying script is stored exactly as given — never sanitised, never
        // refused for its contents. What makes it safe is how it is SERVED; see
        // `crates/gw-api/src/routes/attachments.rs`.
        let dangerous = &b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\">\
                           <script>alert(2)</script></svg>"[..];
        let stored_blob = write(&store, dangerous).await.expect("stored as given");
        assert_eq!(stored_blob.media_type, "image/svg+xml");
        assert_eq!(
            std::fs::read(store.path_for(&stored_blob.sha256).unwrap()).unwrap(),
            dangerous,
            "the bytes on the mount are the bytes that arrived, script and all"
        );
    }

    #[tokio::test]
    async fn a_signature_decides_before_the_text_check_ever_runs() {
        // `%PDF-` is perfectly good text, and these bytes are a PDF. A format that makes a
        // statement about itself is taken at its word before anything is inferred.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert_eq!(
            typed(accept(&store, b"%PDF-1.7\nnothing but text in here\n").await),
            Some("application/pdf")
        );
    }

    #[tokio::test]
    async fn markup_is_stored_as_text_rather_than_refused_or_believed() {
        // The deliberate consequence of accepting text, and the reason it is safe: Markdown
        // may legitimately contain HTML, so refusing HTML would refuse Markdown. What stops
        // it being dangerous is that the wiki never calls it HTML — `text/plain`, `nosniff`
        // and a `Content-Disposition: attachment` are what the browser is handed.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert_eq!(
            typed(accept(&store, b"<!doctype html><script>alert(1)</script>").await),
            Some("text/plain; charset=utf-8")
        );
    }

    #[tokio::test]
    async fn text_is_decided_over_every_byte_and_not_over_the_first_ones() {
        // The difference in kind between this and a signature. A file whose first kilobyte
        // is a licence header and whose remainder is a binary payload is not text, and only
        // a check that sees the whole stream can say so.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let mut bytes = b"# Lizenz\n\nDieser Text ist harmlos.\n".repeat(40);
        bytes.extend_from_slice(&[0x00, 0xff, 0xfe, 0x00]);
        assert_eq!(
            refusal(accept(&store, &bytes).await),
            refusal(BlobOutcome::UnknownType)
        );
        assert!(stored(&store).is_empty());
        assert_eq!(temporaries(&store), 0);
    }

    #[tokio::test]
    async fn a_character_split_across_two_chunks_is_still_one_character() {
        // An upload arrives in pieces and a multi-byte character does not respect them. The
        // scan carries the tail of an incomplete sequence across the boundary; without that
        // every chunked upload of non-ASCII text would be refused as "not UTF-8".
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let text = "Gr\u{f6}\u{df}e und Ma\u{df}e \u{2014} \u{1f600}".as_bytes();
        for split in 1..text.len() {
            let (head, tail) = text.split_at(split);
            assert_eq!(
                typed(accept_chunks(&store, &[head, tail]).await),
                Some("text/plain; charset=utf-8"),
                "split at {split}"
            );
        }
    }

    #[tokio::test]
    async fn a_file_that_ends_mid_character_is_not_text() {
        // The other side of carrying the tail: an incomplete sequence at the END of the
        // upload is a truncated file, not a chunk boundary, and must not be accepted.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert_eq!(
            refusal(accept_chunks(&store, &[b"Gr", b"\xc3"]).await),
            refusal(BlobOutcome::UnknownType)
        );
    }

    #[tokio::test]
    async fn a_byte_order_mark_is_accepted_and_never_removed() {
        // Nothing on this path may alter the bytes: the digest IS the address, so a store
        // that stripped a BOM would hand back a file whose hash the uploader cannot
        // reproduce from their own copy.
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let with_bom = b"\xef\xbb\xbfHallo\n";
        let blob = write(&store, with_bom)
            .await
            .expect("a BOM is not a refusal");
        assert_eq!(blob.media_type, "text/plain; charset=utf-8");
        assert_eq!(blob.byte_size, with_bom.len() as i64);
        assert_eq!(
            std::fs::read(store.path_for(&blob.sha256).unwrap()).unwrap(),
            with_bom
        );
        // UTF-16 carries a byte order mark too, and is not UTF-8: refused rather than
        // stored as text nobody could read.
        assert_eq!(
            refusal(accept(&store, b"\xff\xfeH\x00a\x00l\x00l\x00o\x00").await),
            refusal(BlobOutcome::UnknownType)
        );
    }
}
