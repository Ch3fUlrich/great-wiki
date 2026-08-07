---
title: Wer was sieht
type: page
visibility: public
language: de
sort_key: 4
---

# Wer was sieht

Es gibt drei Sichtbarkeitsstufen, und die Vorgabe ist die strengste.

## Die drei Stufen

- **public** — für alle lesbar, auch ohne Anmeldung
- **internal** — für alle Angemeldeten
- **restricted** — nur mit ausdrücklicher Berechtigung

## Die Vorgabe ist „restricted"

Eine Seite ohne Angabe wird **nicht** veröffentlicht. Das ist Absicht: ein
vergessenes Feld darf niemals etwas offenlegen.

Die Nachbarseite **Nur intern** trägt keine Sichtbarkeit. Sie ist deshalb
`restricted` — nicht öffentlich, obwohl nichts Gegenteiliges dasteht.

## Was du gerade nicht siehst

Du siehst im Moment **alles**, weil die Anwendung noch mit einer
Entwickler-Identität läuft und du über Authelia hereingekommen bist.

Das ist genau der Zustand, den der nächste Abschnitt beendet: eigene Anmeldung,
Berechtigungen je Person, echte Zuordnung von Änderungen — und danach kann die
vorübergehende Sperre am Rand wieder verschwinden.

## Wo die Grenze schon echt ist

Die Prüfung passiert dort, wo gelesen wird — nicht in der Oberfläche. Eine
gesperrte Seite fehlt bereits in der Navigation, nicht nur beim Öffnen. Ein
Titel in einer Menüleiste ist auch dann eine Offenlegung, wenn der Inhalt
geschützt ist.
