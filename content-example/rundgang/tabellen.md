---
title: Tabellen — was heute passiert
type: page
visibility: public
language: de
sort_key: 3
tags: [Rundgang/Tabellen, Format]
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

Diese Tabelle hat zwei Zeilen und bekommt deshalb **keine** Bedienelemente. Ein
Suchfeld über zwei Zeilen ist Lärm vor dem Inhalt: man sieht ohnehin alles.

## Sortieren und filtern

Ab sechs Zeilen bekommt eine Tabelle Bedienelemente — ein Suchfeld für die ganze
Tabelle, ein Filterfeld je Spalte, eine Sortierschaltfläche in jedem Spaltenkopf
und eine Zeilenzählung.

| Probe | Menge | Geprüft | Anteil |
|---|---:|:---:|---:|
| Öl | 900 g | ✅ | 12 % |
| Apfel | 1.200 g | ❌ | 3-5 % |
| Ähre | <0,5 g | ✅ | <0,5 % |
| Zucker | 42 g | ❌ | 80 % |
| Möhre | 1,5 g | ✅ |  |
| Äpfel | 3-5 g | — | 7 % |
| Bohne | 80 g | ✅ | 1,5 % |
| Nuss |  | ❌ | 25 % |

Was beim Sortieren passiert, ist an dieser Tabelle ablesbar:

- **Zahlen sind Zahlen.** *Menge* steigt von `<0,5 g` bis `1.200 g`, nicht von
  `1,5` bis `900` — Einheit, Vergleichszeichen und das deutsche Dezimalkomma
  werden gelesen und dann beiseitegelegt. Ein Bereich wie `3-5 g` zählt mit
  seiner Untergrenze.
- **Umlaute stehen dort, wo ein deutscher Leser sie sucht.** *Probe* beginnt mit
  `Ähre`, nicht mit `Zucker` — nach Zeichencode käme jedes Ä hinter das Z.
- **Leere Zellen stehen immer unten**, in beiden Richtungen. Sie oben zu zeigen
  hieße, die gesuchten Zeilen aus dem Bild zu schieben.
- **Ob eine Spalte aus Zahlen besteht, entscheidet die Spalte**, nicht die
  einzelne Zelle: sonst würde aus einem Namen wie `5-HTP` eine Fünf.
- **Die Sortierung ist stabil.** Erst nach *Geprüft*, dann nach *Probe* sortiert,
  bleibt innerhalb gleicher Namen die vorige Reihenfolge stehen — so entsteht
  eine zweispaltige Sortierung ohne eine Bedienoberfläche dafür.

Die Zeilenzählung steht immer da, auch ungefiltert: *8 von 8 Zeilen*. Eine
gefilterte Tabelle, die vollständig aussieht, ist derselbe Fehler wie eine
Antwort, die verschweigt, was sie weggelassen hat.

Ohne JavaScript erscheint keines dieser Bedienelemente — und die Tabelle steht
trotzdem vollständig da. Ein Filterfeld, das nichts tut, wäre schlimmer als
keines.

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
