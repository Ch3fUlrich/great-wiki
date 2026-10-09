//! The daily email digest over the event bus. Off unless `GW_DIGEST_ENABLED` is exactly
//! `1` or `true`.
//!
//! The wiki owns no mail credential: `SMTP_*` come from the environment only, are never
//! logged, and the config's `Debug` redacts the password. A body names an actor and a page
//! (both read through the delivery filter, [`gw_store::Store::digest_for`]) and never the
//! text of a comment. Rows are marked digested only after a send succeeded.

use anyhow::{bail, Context, Result};
use gw_store::events::{EventKind, Notification};
use gw_store::Store;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::fmt;

/// How many rows one email carries at most.
const DIGEST_LIMIT: usize = 200;

#[derive(Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: String,
    pub user: String,
    pub password: String,
    pub from: String,
}

impl fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("from", &self.from)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestConfig {
    pub smtp: SmtpConfig,
    /// Hour of the day (UTC) after which the day's digest goes out.
    pub hour: u8,
}

/// Read the digest settings from the environment.
pub fn digest_from_env() -> Result<Option<DigestConfig>> {
    let read = |n: &str| {
        std::env::var(n)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    digest_from(
        read("GW_DIGEST_ENABLED"),
        read("GW_DIGEST_HOUR"),
        read("SMTP_HOST"),
        read("SMTP_USER"),
        read("SMTP_PASSWORD"),
        read("SMTP_FROM"),
    )
}

/// The rule, separate from the environment. Disabled reads and validates nothing else.
pub fn digest_from(
    enabled: Option<String>,
    hour: Option<String>,
    host: Option<String>,
    user: Option<String>,
    password: Option<String>,
    from: Option<String>,
) -> Result<Option<DigestConfig>> {
    if !matches!(enabled.as_deref(), Some("1") | Some("true")) {
        return Ok(None);
    }
    let missing: Vec<&str> = [
        ("SMTP_HOST", &host),
        ("SMTP_USER", &user),
        ("SMTP_PASSWORD", &password),
        ("SMTP_FROM", &from),
    ]
    .iter()
    .filter(|(_, v)| v.is_none())
    .map(|(n, _)| *n)
    .collect();
    if !missing.is_empty() {
        bail!(
            "GW_DIGEST_ENABLED is set but the mail settings are incomplete — missing {}",
            missing.join(", ")
        );
    }
    let hour = match hour {
        None => 7,
        Some(h) => h
            .parse::<u8>()
            .ok()
            .filter(|h| *h < 24)
            .with_context(|| format!("GW_DIGEST_HOUR must be 0-23 (got `{h}`)"))?,
    };
    let from = from.unwrap_or_default();
    from.parse::<Mailbox>()
        .with_context(|| "SMTP_FROM is not a valid mailbox")?;
    Ok(Some(DigestConfig {
        smtp: SmtpConfig {
            host: host.unwrap_or_default(),
            user: user.unwrap_or_default(),
            password: password.unwrap_or_default(),
            from,
        },
        hour,
    }))
}

/// Something that can deliver one plain-text message.
pub trait Mailer {
    fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    /// Port 587 with STARTTLS (required, not opportunistic).
    pub fn new(cfg: &SmtpConfig) -> Result<Self> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
            .context("SMTP_HOST is not usable")?
            .port(587)
            .credentials(Credentials::new(cfg.user.clone(), cfg.password.clone()))
            .build();
        Ok(Self {
            transport,
            from: cfg
                .from
                .parse()
                .context("SMTP_FROM is not a valid mailbox")?,
        })
    }

    /// Unencrypted, unauthenticated transport. Only for tests against a local sink.
    #[cfg(test)]
    fn plain_for_test(host: &str, port: u16, from: &str) -> Self {
        Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(port)
                .build(),
            from: from.parse().unwrap(),
        }
    }
}

fn message(from: &Mailbox, to: &str, subject: &str, body: &str) -> Result<Message> {
    Message::builder()
        .from(from.clone())
        .to(to.parse().context("recipient address is not valid")?)
        .subject(subject)
        .body(body.to_string())
        .context("could not build the message")
}

impl Mailer for SmtpMailer {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<()> {
        let msg = message(&self.from, to, subject, body)?;
        // The error text is lettre's own and carries no credential.
        self.transport.send(msg).await.context("SMTP send failed")?;
        Ok(())
    }
}

const SUBJECT: &str = "Ihre Zusammenfassung aus dem Wiki";

fn heading(kind: EventKind) -> &'static str {
    match kind {
        EventKind::CommentReply => "Antworten auf Ihre Kommentare",
        EventKind::Mention => "Erwähnungen",
        EventKind::PageEdited => "Geänderte Seiten",
        EventKind::TaskAssigned => "Dir zugewiesene Aufgaben",
        EventKind::TaskDue => "Fällige Aufgaben",
        EventKind::InviteAccepted => "Angenommene Einladungen",
        EventKind::GrantChanged => "Geänderte Berechtigungen",
    }
}

const ORDER: [EventKind; 7] = [
    EventKind::Mention,
    EventKind::CommentReply,
    EventKind::TaskAssigned,
    EventKind::TaskDue,
    EventKind::PageEdited,
    EventKind::InviteAccepted,
    EventKind::GrantChanged,
];

/// One line of a plain-text mail: every control character, Unicode line or paragraph
/// separator, and invisible format character that could reorder or hide text becomes a
/// space, so a title, path or name cannot start a line of its own.
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control()
                || c == '\u{2028}'
                || c == '\u{2029}'
                || ('\u{200B}'..='\u{200F}').contains(&c)
                || ('\u{202A}'..='\u{202E}').contains(&c)
                || ('\u{2060}'..='\u{2064}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
                || c == '\u{FEFF}'
            {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// German plain text, one section per kind; a line is the actor and the page, nothing else.
pub fn build_body(items: &[Notification]) -> String {
    let mut out =
        String::from("Hallo,\n\nseit der letzten Zusammenfassung ist Folgendes passiert:\n");
    for kind in ORDER {
        let rows: Vec<&Notification> = items.iter().filter(|n| n.kind == kind).collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{}\n", heading(kind)));
        for n in rows {
            let page = match &n.page.title {
                Some(t) => format!("{} ({})", one_line(t), one_line(&n.page.path)),
                None => one_line(&n.page.path),
            };
            match &n.actor_name {
                Some(who) => out.push_str(&format!("- {}: {page}\n", one_line(who))),
                None => out.push_str(&format!("- {page}\n")),
            }
        }
    }
    out
}

/// Send each addressed recipient one mail of what they may still see and have not been
/// told. Rows are marked digested only after a successful send; one recipient's failure
/// does not stop the rest. Returns the number of mails sent.
pub async fn run_digest_once(store: &Store, mailer: &impl Mailer, _now: &str) -> Result<usize> {
    let mut sent = 0;
    for who in store.digest_recipients().await? {
        let Some(to) = who.email.clone() else {
            continue;
        };
        let items = match store.digest_for(&who, DIGEST_LIMIT).await {
            Ok(i) => i,
            Err(error) => {
                tracing::warn!(%error, "digest could not be assembled for one recipient");
                continue;
            }
        };
        if items.is_empty() {
            continue;
        }
        match mailer.send(&to, SUBJECT, &build_body(&items)).await {
            Ok(()) => {
                let ids: Vec<String> = items.iter().map(|n| n.id.clone()).collect();
                store.mark_digested(&who, &ids).await?;
                sent += 1;
            }
            Err(error) => {
                tracing::warn!(%error, "digest not sent; will be retried on the next run");
            }
        }
    }
    Ok(sent)
}

fn utc_now() -> (String, u8) {
    let t = time::OffsetDateTime::now_utc();
    (
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.year(),
            u8::from(t.month()),
            t.day(),
            t.hour(),
            t.minute(),
            t.second()
        ),
        t.hour(),
    )
}

/// Background tick: once a day, at or after the configured hour.
pub async fn run_forever(store: std::sync::Arc<Store>, mailer: SmtpMailer, hour: u8) {
    let mut last_day = String::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        tick.tick().await;
        let (now, h) = utc_now();
        let day = now[..10].to_string();
        if h < hour || day == last_day {
            continue;
        }
        last_day = day;
        if let Err(error) = store.emit_tasks_due(&now, 24).await {
            tracing::warn!(%error, "due-task events not recorded");
        }
        match run_digest_once(&store, &mailer, &now).await {
            Ok(n) => tracing::info!(sent = n, "daily digest run"),
            Err(error) => tracing::warn!(%error, "daily digest run failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};
    use gw_store::events::NewEvent;
    use gw_store::{Author, NewDocument};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        sent: Mutex<Vec<(String, String)>>,
        fail: bool,
    }
    impl Mailer for Fake {
        async fn send(&self, to: &str, _s: &str, body: &str) -> Result<()> {
            if self.fail {
                bail!("down");
            }
            self.sent.lock().unwrap().push((to.into(), body.into()));
            Ok(())
        }
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    async fn page(store: &Store, title: &str) -> (String, String) {
        let body = Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        };
        let id = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
        ("/geheimseite".to_string(), id)
    }

    async fn user(store: &Store, name: &str, mail: Option<&str>) -> Principal {
        store
            .create_local_principal(name, name, mail, "hash")
            .await
            .unwrap()
    }

    async fn mention(store: &Store, to: &Principal, by: &Principal, doc: &str) {
        store
            .emit_event(&NewEvent {
                kind: EventKind::Mention,
                recipient: to.id.clone(),
                actor: Some(by.id.clone()),
                doc_id: Some(doc.into()),
                path: None,
                subject: Some("SECRET COMMENT TEXT".into()),
                dedupe_key: None,
            })
            .await
            .unwrap();
    }

    async fn world() -> (Store, Principal, Principal, Principal, String, String) {
        let st = Store::open("sqlite::memory:").await.unwrap();
        let (path, id) = page(&st, "Geheimseite").await;
        let a = user(&st, "anna", None).await;
        let b = user(&st, "bert", Some("bert@example.org")).await;
        let c = user(&st, "carla", Some("carla@example.org")).await;
        for p in [&b, &c] {
            st.add_grant(&path, Subject::Principal(p.id.clone()), Permission::Read)
                .await
                .unwrap();
        }
        (st, a, b, c, path, id)
    }

    #[tokio::test]
    async fn one_mail_per_recipient_then_nothing_more() {
        let (st, a, b, c, _p, id) = world().await;
        mention(&st, &b, &a, &id).await;
        mention(&st, &c, &a, &id).await;
        let m = Fake::default();
        assert_eq!(run_digest_once(&st, &m, "x").await.unwrap(), 2);
        {
            let sent = m.sent.lock().unwrap();
            assert_eq!(sent.len(), 2);
            for (_, body) in sent.iter() {
                assert!(body.contains("Erwähnungen"));
                assert!(body.contains("anna: Geheimseite (/geheimseite)"));
                assert!(!body.contains("SECRET COMMENT TEXT"));
            }
        }
        assert_eq!(run_digest_once(&st, &m, "x").await.unwrap(), 0);
        assert_eq!(m.sent.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_lost_grant_leaves_nothing_to_send() {
        let (st, a, b, c, path, id) = world().await;
        mention(&st, &b, &a, &id).await;
        mention(&st, &c, &a, &id).await;
        st.remove_grant(&path, &Subject::Principal(b.id.clone()), Permission::Read)
            .await
            .unwrap();
        let m = Fake::default();
        assert_eq!(run_digest_once(&st, &m, "x").await.unwrap(), 1);
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent[0].0, "carla@example.org");
        assert!(!sent.iter().any(|(to, _)| to == "bert@example.org"));
    }

    #[tokio::test]
    async fn a_failed_send_leaves_rows_undigested() {
        let (st, a, b, _c, _p, id) = world().await;
        mention(&st, &b, &a, &id).await;
        let bad = Fake {
            fail: true,
            ..Default::default()
        };
        assert_eq!(run_digest_once(&st, &bad, "x").await.unwrap(), 0);
        assert_eq!(st.digest_for(&b, 10).await.unwrap().len(), 1);
        let ok = Fake::default();
        assert_eq!(run_digest_once(&st, &ok, "x").await.unwrap(), 1);
    }

    #[test]
    fn disabled_builds_nothing_and_ignores_the_rest() {
        assert!(digest_from(None, None, None, None, None, None)
            .unwrap()
            .is_none());
        assert!(digest_from(s("yes"), s("99"), None, None, None, None)
            .unwrap()
            .is_none());
        assert!(
            digest_from(s("0"), None, s("h"), s("u"), s("p"), s("a@b.c"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn enabled_with_incomplete_smtp_refuses() {
        let err = digest_from(s("1"), None, s("h"), None, s("p"), s("wiki@example.org"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("SMTP_USER"));
        assert!(!err.contains("SMTP_HOST"));
    }

    #[test]
    fn enabled_complete_defaults_to_seven() {
        let c = digest_from(
            s("true"),
            None,
            s("h"),
            s("u"),
            s("p"),
            s("wiki@example.org"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.hour, 7);
        assert!(digest_from(
            s("1"),
            s("24"),
            s("h"),
            s("u"),
            s("p"),
            s("wiki@example.org")
        )
        .is_err());
    }

    #[test]
    fn debug_never_shows_the_password() {
        let c = digest_from(
            s("1"),
            None,
            s("h"),
            s("u"),
            s("hunter2-not-real"),
            s("wiki@example.org"),
        )
        .unwrap()
        .unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("hunter2-not-real"));
        assert!(shown.contains("redacted"));
    }

    #[tokio::test]
    async fn smtp_implementation_speaks_to_a_local_sink() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let sink = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut r = BufReader::new(r);
            w.write_all(b"220 sink ESMTP\r\n").await.unwrap();
            let (mut data, mut in_data, mut seen) = (String::new(), false, String::new());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                if in_data {
                    if line == ".\r\n" {
                        in_data = false;
                        w.write_all(b"250 queued\r\n").await.unwrap();
                    } else {
                        data.push_str(&line);
                    }
                    continue;
                }
                let up = line.to_ascii_uppercase();
                seen.push_str(&up);
                if up.starts_with("EHLO") {
                    w.write_all(b"250 sink\r\n").await.unwrap();
                } else if up.starts_with("DATA") {
                    in_data = true;
                    w.write_all(b"354 go\r\n").await.unwrap();
                } else if up.starts_with("QUIT") {
                    w.write_all(b"221 bye\r\n").await.unwrap();
                    break;
                } else {
                    w.write_all(b"250 ok\r\n").await.unwrap();
                }
            }
            (data, seen)
        });
        let m = SmtpMailer::plain_for_test("127.0.0.1", port, "wiki@example.org");
        m.send("bert@example.org", SUBJECT, "- anna: Seite\n")
            .await
            .unwrap();
        let (data, seen) = sink.await.unwrap();
        assert!(seen.contains("RCPT TO:<BERT@EXAMPLE.ORG>"));
        assert!(data.contains("Subject:"));
        assert!(data.contains("anna: Seite"));
    }

    fn note(title: Option<&str>, path: &str, actor: Option<&str>) -> Notification {
        Notification {
            id: "n1".into(),
            kind: EventKind::Mention,
            created_at: "2026-01-01 00:00:00".into(),
            read: false,
            actor_name: actor.map(|a| a.to_string()),
            page: gw_store::events::NotificationPage {
                path: path.into(),
                title: title.map(|t| t.to_string()),
            },
            subject: None,
        }
    }

    #[test]
    fn a_newline_in_the_title_cannot_add_a_line() {
        let body = build_body(&[note(Some("X\n- admin: forged"), "/seite", Some("anna"))]);
        assert!(body.contains("- anna: X - admin: forged (/seite)"));
        assert!(!body.contains("X\n- admin"));
        assert_eq!(body.lines().count(), 6);
    }

    #[test]
    fn a_crlf_in_the_actor_name_cannot_add_a_line() {
        let body = build_body(&[note(
            Some("Seite"),
            "/seite",
            Some("bob\r\n- admin: forged"),
        )]);
        assert!(body.contains("- bob  - admin: forged: Seite (/seite)"));
        assert!(!body.contains("bob\r\n- admin"));
        assert_eq!(body.lines().count(), 6);
    }

    #[test]
    fn a_unicode_line_separator_in_the_path_cannot_add_a_line() {
        let body = build_body(&[note(
            Some("Seite"),
            "/a\u{2028}- admin: forged",
            Some("anna"),
        )]);
        assert!(body.contains("- anna: Seite (/a - admin: forged)"));
        assert!(!body.contains("\u{2028}"));
        assert_eq!(body.lines().count(), 6);
    }

    #[test]
    fn an_ordinary_title_is_unchanged() {
        let body = build_body(&[note(Some("Geheimseite"), "/geheimseite", Some("anna"))]);
        assert!(body.contains("- anna: Geheimseite (/geheimseite)"));
    }

    #[test]
    fn right_to_left_override_in_title_becomes_space() {
        let body = build_body(&[note(Some("X\u{202E}Y"), "/seite", Some("anna"))]);
        assert!(body.contains("- anna: X Y (/seite)"));
        assert!(!body.contains('\u{202E}'));
        assert_eq!(body.lines().count(), 6);
    }

    #[test]
    fn zero_width_space_in_title_becomes_space() {
        let body = build_body(&[note(Some("X\u{200B}Y"), "/seite", Some("anna"))]);
        assert!(body.contains("- anna: X Y (/seite)"));
        assert!(!body.contains('\u{200B}'));
        assert_eq!(body.lines().count(), 6);
    }

    #[test]
    fn accented_german_title_is_unchanged() {
        let body = build_body(&[note(Some("Übersicht für Ärzte"), "/seite", Some("anna"))]);
        assert!(body.contains("- anna: Übersicht für Ärzte (/seite)"));
    }
}
