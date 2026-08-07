---
title: Erste Schritte
type: page
visibility: public
sort_key: 10
---

# Erste Schritte

Diese Seite liegt unter `handbuch/`, bekommt dadurch `parent_path = /handbuch` und
erscheint im Baum als Kind des Handbuchs.

## Voraussetzungen

- Rust (die Version aus `rust-toolchain.toml`)
- Node 24.19.0 für die Weboberfläche
- Kein Datenbankserver: SQLite liegt unter `data/`

## Der erste Start

1. Inhalte laden
2. Server starten
3. Baum abrufen

```sh
cargo run -p gw-api -- seed --content content-example
cargo run -p gw-api -- serve
curl -s localhost:8092/api/tree
```

Der Server bindet standardmäßig auf `127.0.0.1:8092`. Port 8090 wird bewusst abgelehnt —
dort läuft bereits `omnigraph-viewer`.

> Ohne gesetztes `GW_PROXY_SECRET` verweigert ein nicht-loopback Bind den Start. Das ist
> Absicht: Caddy läuft auf einem anderen Host, der Port ist also im LAN erreichbar.
