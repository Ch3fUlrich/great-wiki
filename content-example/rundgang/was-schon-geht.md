---
title: Was schon geht
type: page
visibility: public
language: de
sort_key: 1
---

# Was schon geht

Diese Seite ist der Beweis. Jeder Absatz hier wurde aus einer Markdown-Datei
gelesen, in Blöcke umgewandelt, in einer Datenbank gespeichert und wieder
gerendert.

## Überschriften bauen die Gliederung

Rechts (oder unten auf dem Handy) steht ein Inhaltsverzeichnis. Es wird nicht von
Hand gepflegt, sondern beim Lesen aus den Überschriften gewonnen.

### Auch verschachtelt

Die Ebene wird mitgeführt und im Inhaltsverzeichnis eingerückt.

## Listen

- Aufzählungen funktionieren
- Auch mit mehreren Punkten
- Die Reihenfolge bleibt erhalten

1. Nummerierte Listen ebenso
2. Zweiter Punkt
3. Dritter Punkt

## Zitate

> Ein Zitat wird als eigener Blocktyp gespeichert, nicht als Absatz mit
> besonderem Aussehen. Das ist der Unterschied zwischen Struktur und Formatierung.

## Code

```rust
pub fn slugify(input: &str) -> String {
    // Deutsche Zeichen werden VOR der ASCII-Faltung ersetzt.
    // Sonst wird aus "Präbiotika" das unbrauchbare "pr-biotika".
}
```

## Wo die Grenze liegt

**Fett** und *kursiv* siehst du hier nicht — der Text bleibt erhalten, die
Auszeichnung wird noch verworfen. Das Ladeprogramm sagt das beim Import auch
ausdrücklich, statt es stillschweigend zu tun.
