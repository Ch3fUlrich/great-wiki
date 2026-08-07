/// Turn arbitrary text into a URL-safe slug.
///
/// German characters are transliterated *before* ASCII folding (ä→ae, ö→oe, ü→ue, ß→ss).
/// Without that step every German title collapses into a mangled slug — "Präbiotika"
/// becomes "pr-biotika" — which is both ugly and lossy, since "Präbiotika" and "Prabiotika"
/// would produce different slugs while "Präbiotika" and "Prbiotika" would collide.
///
/// Output is ASCII-only so slugs are stable across filesystems that normalise Unicode
/// differently (macOS NFD vs Linux NFC) and safe in a URL without percent-encoding.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // Starts true so a leading separator run produces no leading dash.
    let mut last_dash = true;

    for ch in input.chars() {
        let expansion = match ch {
            'ä' | 'Ä' => "ae",
            'ö' | 'Ö' => "oe",
            'ü' | 'Ü' => "ue",
            'ß' | 'ẞ' => "ss",
            _ => "",
        };

        if !expansion.is_empty() {
            out.push_str(expansion);
            last_dash = false;
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            // Any run of non-alphanumerics becomes exactly one dash.
            out.push('-');
            last_dash = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::slug::slugify;

    #[test]
    fn lowercases_and_collapses_separators() {
        assert_eq!(
            slugify("  Table 0:  Dysbiotic   Shifts!! "),
            "table-0-dysbiotic-shifts"
        );
    }

    #[test]
    fn strips_em_dashes_without_leaving_double_separators() {
        assert_eq!(
            slugify("Darm — ADHD Microbiota Reference"),
            "darm-adhd-microbiota-reference"
        );
    }

    // The predecessor plan asserted slugify("Präbiotika") == "pr-biotika". That is a
    // mangled URL for every German title, so transliteration is a requirement, not a nicety.
    #[test]
    fn transliterates_german_umlauts() {
        assert_eq!(slugify("Präbiotika Guide"), "praebiotika-guide");
        assert_eq!(slugify("Größe und Maß"), "groesse-und-mass");
        assert_eq!(slugify("Öl Überblick"), "oel-ueberblick");
    }

    #[test]
    fn drops_characters_with_no_ascii_equivalent() {
        assert_eq!(slugify("Präbiotika 🧬 Guide"), "praebiotika-guide");
    }

    #[test]
    fn empty_and_separator_only_input_yield_empty() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("---   ---"), "");
    }
}
