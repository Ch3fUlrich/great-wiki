---
title: Größe und Maß — Deutsch im System
type: research
visibility: public
language: de
sort_key: 2
---

# Größe und Maß — Deutsch im System

Schau in die Adresszeile. Diese Seite heißt *Größe und Maß*, ihre Adresse lautet
aber `/rundgang/groesse-und-mass`.

## Warum das wichtig ist

Der naheliegende Weg wäre, alles wegzuwerfen, was nicht ASCII ist. Genau das tat
der ursprüngliche Entwurf — und machte aus `Präbiotika` das Wort `pr-biotika`.

Das ist nicht nur hässlich, sondern verlustbehaftet: `Präbiotika` und
`Prbiotika` würden auf dieselbe Adresse zeigen und einander überschreiben.

## Was stattdessen passiert

| Eingabe | Adresse |
|---|---|
| Größe und Maß | groesse-und-mass |
| Präbiotika Guide | praebiotika-guide |
| Öl Überblick | oel-ueberblick |

Umlaute werden ersetzt, bevor gefaltet wird: ä→ae, ö→oe, ü→ue, ß→ss.

## Zweimal geprüft

Dieselbe Regel gibt es zweimal — einmal in Rust auf dem Server, einmal in
TypeScript im Browser. Beide teilen sich dieselben Testfälle, damit sie nicht
auseinanderlaufen können.
