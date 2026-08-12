---
title: Import und Export
type: page
visibility: public
slug: import-export
language: de
sort_key: 6
---

# Import und Export

Der Inhalt liegt in der Datenbank. Markdown ist das Austauschformat — der Weg hinein und
der Weg heraus, nicht der Speicherort.

## Zwei Befehle

```sh
great-wiki seed   --content ./inhalt
great-wiki export --content ./inhalt --as anna
```

`seed` liest ein Verzeichnis ein, `export` schreibt eines. Die Ordnerstruktur ist der
Seitenbaum: `rundgang/tabellen.md` wird zur Seite `/rundgang/tabellen`, und der Ordner
`rundgang/` gehört der Datei `rundgang.md` daneben.

## Was der Titel dieser Seite verrät

Diese Seite heißt *Import und Export*, ihre Adresse lautet aber `/rundgang/import-export`.
Das steht so im Kopf der Datei:

| Feld | Wert | Wirkung |
|---|---|---|
| title | Import und Export | was oben auf der Seite steht |
| slug | import-export | was in der Adresszeile steht |

Ein ausdrücklicher `slug` gewinnt gegen den Titel. Das ist wichtig für den Rückweg: der
Export benennt jede Datei nach dem `slug` und schreibt ihn auch in den Dateikopf. Täte er
das nicht, käme die Seite beim nächsten Einlesen unter einer anderen Adresse an — und alle
Unterseiten hingen an einer Adresse, die es nicht mehr gibt.

## Aktualisieren ist ein eigener Schalter

Ohne weitere Angabe ist eine schon vorhandene Seite ein **Fehler**, keine Überschreibung.
Wer sie wirklich ändern will, sagt das:

```sh
great-wiki seed --content ./inhalt --as anna --update
```

Dann steht am Ende genau da, was passiert ist: angelegt, geändert, unverändert,
übersprungen. Eine unveränderte Seite wird gar nicht erst angefasst — ein wirkungsloser
Eintrag in der Versionsgeschichte verdeckt nur die echten Änderungen.

Drei Dinge tut auch `--update` nicht:

1. **Löschen.** Eine Seite, zu der keine Datei mehr existiert, wird genannt und bleibt
   stehen. Löschen ist ein anderes Wort und braucht einen eigenen Befehl.
2. **Verschieben oder veröffentlichen.** Titel, Typ, Sichtbarkeit, Sprache und Reihenfolge
   werden gemeldet und nicht übernommen. Ein vergessenes `visibility: public` in einer
   Datei darf keine Seite ins Netz stellen.
3. **Ohne Namen arbeiten.** Jede Änderung wird als Version geschrieben und trägt den
   Namen des Kontos, das sie gemacht hat — mit derselben Rechteprüfung wie eine Änderung
   im Browser.

## Was der Export nicht enthalten kann

> Die Datenbank kennt keine Auszeichnung im Fließtext: kein fett, kein kursiv, keine
> Verweise, keine Bilder. Diese Angaben gehen schon beim Einlesen verloren — der Import
> sagt das Zeile für Zeile — und können deshalb auch nicht wieder herauskommen.

Alles andere kommt genau so zurück, wie es hineingegangen ist. Der Export prüft das für
jede Seite einzeln: er wandelt seine eigene Ausgabe zurück und vergleicht. Was dabei nicht
gleich herauskommt, wird **nicht geschrieben** und namentlich gemeldet.

Deshalb steht neben den Dateien immer eine `EXPORT-README.txt` mit demselben Satz. Eine
Warnung, die nur im Terminal stand, ist beim nächsten Öffnen des Ordners nicht mehr da.
