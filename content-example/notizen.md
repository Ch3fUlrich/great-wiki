---
title: Notizen
type: page
sort_key: 20
---

# Notizen

Diese Datei nennt **kein** `visibility`. Genau deshalb steht sie hier: sie landet als
`restricted` in der Datenbank, nicht als `public`.

Ein vergessenes Feld darf niemals etwas veröffentlichen. Der Vorgabewert ist der private,
und zwar im Typ selbst (`Visibility::default()`), nicht in einer Prüfung, die man an einer
Stelle vergessen kann.

## Woran man das sieht

Ein anonymer Aufruf von `/api/tree` zeigt diese Seite nicht — nicht einmal den Titel. Ein
eingeschränkter Titel in der Navigation wäre bereits eine Preisgabe, auch ohne Inhalt.
