---
title: Tabellen — was heute passiert
type: page
visibility: public
language: de
sort_key: 3
---

# Tabellen — was heute passiert

Diese Seite zeigt absichtlich etwas, das noch **nicht** fertig ist.

## Die Tabelle

| Feld | Wert | Einheit |
|---|---|---|
| Länge | 42 | Meter |
| Breite | 7 | Meter |

Oben siehst du keine Tabelle, sondern Absätze — einen je Zeile. Der Blocktyp
`table` existiert noch nicht, also wird der Text platt gemacht statt verworfen.

## Warum das die richtige Entscheidung ist

Inhalt still zu verlieren ist der einzige wirklich inakzeptable Ausgang. Ein
Import, der sagt *„hier war eine Tabelle, ich habe den Text behalten"*, kann
später repariert werden. Ein Import, der schweigt, kann das nicht.

Beim Laden erscheint dazu diese Zeile:

```
note  tabellen.md: 1× table (cell text flattened into one paragraph per row)
```

## Ein Fehler, den das fast verdeckt hätte

Zellen wurden zunächst direkt aneinandergehängt: aus `Länge | Meter` wurde
`LängeMeterm`. Der Test dafür prüfte `enthält "Feld"` — und das gilt für
`FeldWert` genauso.

Die Lehre gilt allgemein: **eine Zusicherung über extrahierten Text muss exakt
sein, niemals eine Teilzeichenkette.** Genau dagegen ist eine Teilprüfung blind.
