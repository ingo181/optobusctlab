# optobusctlab

Rust-Nachfolger für [JLab](https://sourceforge.net/projects/jlab/) und die
LabVIEW-Demos aus Carsten Meyers **c't-Lab**-Serie (c't 10/2007–19/2007) –
dem modularen Selbstbau-Messsystem für PC-gesteuerte Labortechnik
(Netzteil, Funktionsgenerator, Digitalvoltmeter, I/O-Karten).

Ziel: eine moderne, plattformübergreifende Steuer-/Mess-UI, die

- **auf einem Raspberry Pi mit 7"-Touch** direkt auf dem c't-Lab-Gehäuse läuft
  (Kiosk-Browser gegen einen lokalen Server), und
- **als Desktop-App** (Windows/macOS/Linux) per Installer verteilt werden kann,

ohne die Installations-Hürden von Java+RXTX (JLab) oder einer LabVIEW-Lizenz.

> **Status:** früher Prototyp. Backend-Kern (Protokoll, Transport, Actor)
> steht und ist getestet; Hardware-Anbindung, Web-Frontend und Desktop-Bundle
> fehlen noch. Siehe [`CLAUDE.md`](./CLAUDE.md) für den aktuellen
> Architektur-Stand und die nächsten Schritte.

## Warum "optobusctlab"?

**OptoBus** ist der Name, den die Original-Artikel für den optoisolierten
seriellen Ring verwenden, der alle c't-Lab-Module verbindet. **ctlab** ist
die Kurzform, unter der das Projekt seit den LabVIEW-Demos bekannt ist
(Ressourcenname `CTLAB` u.a.). Beides zusammen, damit die Herkunft erkennbar
bleibt.

## Architektur

Vierschichtig, angelehnt an die C#-"CtLab Library" von Volker Raum:

```
crates/
  octlab-transport   Verbindungsebene (seriell / TCP / simuliert)
  octlab-protocol    c't-Lab-ASCII-Protokoll (Parsen & Bauen von Kommandos)
  octlab-devices     typisierte Geräte-Abstraktion (DDS, künftig DCG/DIV/ADA-IO/…)
  octlab-lab         zentraler Actor: Sync-Query mit Timeout + Live-Broadcast
  octlab-server      Axum-HTTP/WebSocket-Server über octlab-lab
```

Details, Design-Entscheidungen und die Reihenfolge der nächsten Schritte
stehen in [`CLAUDE.md`](./CLAUDE.md).

## Bauen & Testen

```bash
cargo test --workspace
cargo run -p octlab-server     # startet auf :3000, läuft ohne Hardware
curl localhost:3000/health
```

## Lizenz

[MIT](./LICENSE). Basiert auf öffentlich dokumentiertem Protokoll-Wissen aus
der c't-Lab-Artikelserie (Heise) sowie der JLab- und CtLab-Library-Doku;
es wird kein Code aus diesen Vorgänger-Projekten übernommen.
