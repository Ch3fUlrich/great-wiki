---
title: Beispielinhalte
type: page
visibility: public
sort_key: 0
---

# Beispielinhalte

Dieses Verzeichnis macht ein frisches Checkout sofort benutzbar. Es enthält gerade genug
Material, um jeden Blocktyp zu zeigen, den `gw-core` heute versteht — und, an einer
Tabelle, genau die Stelle, an der er noch nicht ausreicht.

```sh
cargo run -p gw-api -- seed --content content-example
cargo run -p gw-api -- serve
```

Die Datenbank ist die Quelle der Wahrheit. Diese Dateien sind ein *Importformat*: nach dem
Seeden wird im Editor gearbeitet, nicht hier. `seed` überschreibt deshalb nichts und
bricht mit einem Fehler ab, sobald eine Datei nicht geladen werden konnte.

## Konventionen

- Jede Datei beginnt mit YAML-Frontmatter. `title` ist Pflicht — es wird niemals aus dem
  Dateinamen erraten.
- Ohne `visibility` ist ein Dokument `restricted`. Ein vergessenes Feld veröffentlicht
  nichts.
- Ein Verzeichnis ist kein Dokument. `handbuch/` gehört der Datei `handbuch.md` daneben;
  fehlt sie, werden die Kindseiten übersprungen statt einen Platzhalter zu erfinden.
- Die Reihenfolge im Baum kommt aus `sort_key`, nicht aus dem Dateinamen.
