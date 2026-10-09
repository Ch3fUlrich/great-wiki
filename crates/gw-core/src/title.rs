/// Validate a page title for reader-facing use.
///
/// Returns `Some(German-free English reason)` when the title contains any control or invisible
/// formatting character that should not appear in a visible title, else `None`.
///
/// The check includes:
/// - All characters where `c.is_control()` is true
/// - U+2028 (LINE SEPARATOR) and U+2029 (PARAGRAPH SEPARATOR)
/// - U+200B..=U+200F (ZERO WIDTH SPACE .. ZERO WIDTH NON-JOINER)
/// - U+202A..=U+202E (LEFT/RIGHT TO DIRECTIONAL ISOLATES, EMBEDDING, OVERRIDING)
/// - U+2060..=U+2064 (WORD JOINER, etc.)
/// - U+2066..=U+2069 (LEFT/RIGHT TO, ISOLATE)
/// - U+FEFF (ZERO WIDTH NO-BREAK SPACE)
pub fn title_problem(title: &str) -> Option<&'static str> {
    let bad_chars = title
        .chars()
        .filter(|c| {
            c.is_control()
                || matches!((*c) as u32, 0x2028 | 0x2029)
                || matches!((*c) as u32, 0x200B..=0x200F)
                || matches!((*c) as u32, 0x202A..=0x202E)
                || matches!((*c) as u32, 0x2060..=0x2064)
                || matches!((*c) as u32, 0x2066..=0x2069)
                || *c == '\u{FEFF}'
        })
        .collect::<Vec<_>>();

    if !bad_chars.is_empty() {
        Some("a title may not contain control or invisible formatting characters")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_problem_refuses_control_chars() {
        assert!(title_problem("Seite\nX").is_some());
        assert!(title_problem("Seite\tX").is_some());
        assert!(title_problem("Seite\rX").is_some());
    }

    #[test]
    fn title_problem_refuses_specific_invisible_chars() {
        assert!(title_problem("a\u{202E}b").is_some());
        assert!(title_problem("a\u{200B}b").is_some());
        assert!(title_problem("\u{FEFF}x").is_some());
        assert!(title_problem("a\u{202A}b").is_some());
        assert!(title_problem("a\u{200F}b").is_some());
        assert!(title_problem("a\u{2060}b").is_some());
        assert!(title_problem("a\u{2066}b").is_some());
    }

    #[test]
    fn title_problem_accepts_good_titles() {
        assert!(title_problem("Übersicht für Ärzte").is_none());
        assert!(title_problem("Tabelle 1: Ergebnisse (2026)").is_none());
        assert!(title_problem("Normaler Titel").is_none());
        assert!(title_problem("Titel mit Zahlen 123").is_none());
        assert!(title_problem("Titel mit Satzzeichen! ").is_none());
    }
}
