---
title: Tabellen — was heute passiert
type: page
visibility: public
language: de
sort_key: 3
---

# Tabellen — was heute passiert

Diese Seite zeigte lange etwas, das noch **nicht** fertig war. Seit Kurzem ist es
das: eine Tabelle wird als Tabelle gespeichert und als Tabelle gezeigt.

## Die Tabelle

| Feld | Wert | Einheit |
|---|---:|---|
| Länge | 42 | Meter |
| Breite | 7 | Meter |

Die Spalte *Wert* steht rechtsbündig, weil die Trennzeile im Markdown das so
sagt (`---:`). Die Ausrichtung reist an den Zellen mit, denn sie stillschweigend
zu verwerfen ist genau der Weg, auf dem eine Zahlenspalte ausgefranst ankommt.

Die Kopfzeile ist keine erste Zeile mit anderem Aussehen, sondern eine Zeile aus
Kopfzellen — für einen Screenreader der Unterschied zwischen einer benannten
Spalte und einem Feld ohne Beschriftung.

Ist eine Tabelle breiter als die Seite, scrollt sie in ihrem eigenen Kasten. Die
Seite selbst scrollt nie seitwärts — das ist es, was ein Dokument auf dem Handy
unlesbar macht. Der Kasten ist mit der Tabulatortaste erreichbar, sonst käme man
ohne Maus nicht an den verdeckten Teil.

## Was vorher hier stand

Bis dahin gab es den Blocktyp `table` nicht, und jede Zeile wurde zu einem Absatz
platt gemacht. Beim Laden erschien dazu diese Zeile:

```
note  tabellen.md: 1× table (cell text flattened into one paragraph per row)
```

Das war die richtige Zwischenstufe. Inhalt still zu verlieren ist der einzige
wirklich inakzeptable Ausgang. Ein Import, der sagt *„hier war eine Tabelle, ich
habe den Text behalten"*, kann später repariert werden — und genau das ist
passiert. Ein Import, der schweigt, kann das nicht.

## Ein Fehler, den das fast verdeckt hätte

Zellen wurden zunächst direkt aneinandergehängt: aus `Länge | Meter` wurde
`LängeMeterm`. Der Test dafür prüfte `enthält "Feld"` — und das gilt für
`FeldWert` genauso.

Die Lehre gilt allgemein: **eine Zusicherung über extrahierten Text muss exakt
sein, niemals eine Teilzeichenkette.** Genau dagegen ist eine Teilprüfung blind.
Die Zusicherung steht bis heute im Testlauf, jetzt über die Zellen der echten
Tabelle.
