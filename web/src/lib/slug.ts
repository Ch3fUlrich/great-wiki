// Mirrors crates/gw-core/src/slug.rs. The shared test corpus in slug.test.ts is what
// keeps the two honest; change one and you must change the other.
const TRANSLITERATIONS: Record<string, string> = {
  ä: 'ae', Ä: 'ae',
  ö: 'oe', Ö: 'oe',
  ü: 'ue', Ü: 'ue',
  ß: 'ss', ẞ: 'ss'
};

const ASCII_ALPHANUMERIC = /[A-Za-z0-9]/;

/**
 * Turn arbitrary text into a URL-safe slug.
 *
 * German characters are transliterated before ASCII folding, so "Präbiotika" becomes
 * "praebiotika" rather than the lossy "pr-biotika".
 */
export function slugify(input: string): string {
  let out = '';
  // Starts true so a leading separator run produces no leading dash.
  let lastDash = true;

  for (const ch of input) {
    const expansion = TRANSLITERATIONS[ch];
    if (expansion !== undefined) {
      out += expansion;
      lastDash = false;
    } else if (ASCII_ALPHANUMERIC.test(ch)) {
      out += ch.toLowerCase();
      lastDash = false;
    } else if (!lastDash) {
      out += '-';
      lastDash = true;
    }
  }

  return out.replace(/-+$/, '');
}
