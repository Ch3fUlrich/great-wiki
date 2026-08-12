---
title: Heikler Text — was den Rückweg übersteht
type: research
visibility: public
slug: heikel
language: de
sort_key: 1
---

# Heikler Text — was den Rückweg übersteht

Diese Seite ist eine Falle für den Export. Jeder Abschnitt enthält etwas, das beim
Zurückschreiben nach Markdown als Auszeichnung gelesen werden könnte — und als Text
zurückkommen muss.

## Zeichen, die wie Markdown aussehen

Ein Stern \*so\* und ein Unterstrich \_so\_ sind hier keine Auszeichnung, sondern
Zeichen. Genauso `snake_case_name`, eine eckige Klammer \[so\], ein Winkel \<so\> und
zwei Tilden \~\~so\~\~. Ein Kaufmanns-Und wie \&amp; muss fünf Zeichen bleiben und darf
nicht zu einem werden.

Am Zeilenanfang wird es ernst: die folgenden vier Absätze sind Absätze, keine
Überschriften, Zitate, Listen oder Trennstriche.

\# keine Überschrift

\> kein Zitat

\- keine Aufzählung

1\. keine Nummerierung

## Zahlen, wie sie in einem medizinischen Text vorkommen

Ein Anteil von <0,5 % ist etwas anderes als 5 %, und ein Bereich von 3-5 mg ist etwas
anderes als 35 mg. Ein Verhältnis a|b enthält einen senkrechten Strich, der außerhalb
einer Tabelle nichts zu bedeuten hat.

Das ist der Grund, warum diese Seite existiert: **ein stillschweigend verlorenes
Kleiner-Zeichen ändert eine Aussage über eine Dosis.**

## Tief verschachtelte Listen

- Erste Ebene
  - Zweite Ebene
    - Dritte Ebene
      - Vierte Ebene, und hier hört es nicht auf, weil es nicht mehr geht, sondern weil
        niemand tiefer liest
    - Zurück auf die dritte
- Wieder auf der ersten

1. Nummeriert geht das auch
   1. Und verschachtelt
      - und gemischt
2. Zweiter Punkt

7. Eine Liste, die bei sieben beginnt
8. und bei acht weitergeht

## Eine Tabelle mit Lücken

| Probe | Menge | Anteil | Bemerkung |
|---|---:|:---:|---|
| Öl | 900 g | 12 % | |
| Ähre | <0,5 g | | mit Lücke |
| | 42 g | 3-5 % | ohne Namen |
| Maß | | | fast leer |

Eine leere Zelle behält ihren Platz. Verlöre sie ihn, rutschte jede spätere Zelle eine
Spalte nach links, und die Tabelle sähe dabei völlig unauffällig aus.

## Code, der Code enthält

````md
```rust
pub fn slugify(input: &str) -> String {
    // Größe und Maß → groesse-und-mass
}
```
````

Der äußere Zaun ist länger als der innere. Wäre er es nicht, endete der Block zu früh und
der Rest des Codes stünde als Fließtext auf der Seite.

## Ein Zitat mit Innenleben

> Der erste Absatz des Zitats.
>
> Der zweite Absatz desselben Zitats.
>
> > Und ein Zitat im Zitat, weil auch das eine Verschachtelung ist, die beim Rückweg
> > verlorengehen könnte.
