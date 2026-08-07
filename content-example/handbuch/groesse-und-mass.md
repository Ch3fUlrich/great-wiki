---
title: Größe und Maß
type: research
visibility: internal
language: de
sort_key: 20
---

# Größe und Maß

Deutsche Titel werden transliteriert, bevor sie zu einem Slug werden: aus *Größe und Maß*
wird `groesse-und-mass`. Ohne diesen Schritt hieße die Seite `gr-e-und-ma` — hässlich und
verlustbehaftet, weil unterschiedliche Titel denselben Slug bekämen.

## Umlaute im Fließtext

Äpfel, Öl, Übung, Straße, Maß, Fuß — alle überleben unverändert im Blocktree. Nur der
Pfad ist ASCII, damit er ohne Prozentkodierung in einer URL steht.

## Einheiten

| Größe   | Einheit | Symbol |
| ------- | ------- | ------ |
| Länge   | Meter   | m      |
| Masse   | Kilo    | kg     |
| Zeit    | Sekunde | s      |

Diese Tabelle ist der interessante Teil: `BlockKind` kennt heute keinen Tabellentyp, also
wird jede Zeile zu einem Absatz und jede Zelle behält ihren Text. `seed` meldet das als
Hinweis. M8 ersetzt die Absätze durch echte Tabellenblöcke — der Text war nie weg.

## Sichtbarkeit

Diese Seite ist `internal`: angemeldete Personen sehen sie, anonyme nicht. Sie taucht
deshalb im Baum eines nicht angemeldeten Aufrufs gar nicht erst auf.
