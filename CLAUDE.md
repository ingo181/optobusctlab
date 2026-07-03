# optobusctlab – Projektkontext für Claude Code

**Repo-/Projektname: `optobusctlab`** (GitHub, README, Produktname).
**Interner Crate-Präfix: `octlab`** (kurz für Opto-Bus-ctlab, aus Tipp-Ergonomie
in `use`-Statements – nicht mit dem Repo-Namen verwechseln, das ist bewusst
kürzer). Falls das Projekt wachsen sollte, ist eine spätere Umbenennung auf
den vollen `optobusctlab-*`-Präfix ein reines Suchen/Ersetzen, kein
strukturelles Problem.

## Worum es geht

Steuer-/Mess-UI für Carsten Meyers c't-Lab (Baukasten-Messsystem, c't-Artikelserie
2007). Nachfolger von JLab (Java) und den LabVIEW-Demos aus der Originalserie.
Zielbild: Raspberry Pi mit 7"-Touch auf dem c't-Lab-Gehäuse (Kiosk-Browser) +
Desktop-App via Tauri, gleiche UI, gemeinsamer Rust-Code.

## Architektur (4 Schichten, angelehnt an die C#-"CtLab Library" von J. Raum)

```
crates/
  octlab-transport   Verbindungsebene: BoardConnection-Trait (seriell/TCP/simuliert)
  octlab-protocol    Kommando/Nachricht: c't-Lab-ASCII-Syntax parsen & bauen
  octlab-devices     Geräteebene: typisierte Module (Dds, künftig Dcg/Div/AdaIo/...)
  octlab-lab         Umgebungsebene: Lab-Actor, Sync-Query mit Timeout, Broadcast
  octlab-server      Axum: HTTP/WebSocket, nutzt octlab-lab
apps/               (noch nicht angelegt)
  web/              Leptos-Frontend (WASM)
  desktop/          Tauri-Wrapper um octlab-server + web/
```

Jede Schicht kennt nur die darunterliegende, nie umgekehrt. Neue Module folgen
dem Muster in `octlab-devices/src/lib.rs` (siehe `Dds`-Struct).

## Wichtige Design-Entscheidungen (bitte nicht versehentlich rückgängig machen)

- **Immer volle Adresse senden**, kein "sticky addressing" wie im
  Original-Protokoll (dort eine Bandbreite-Optimierung für einen einzelnen
  seriellen Client aus 2007 – bei uns senden potenziell mehrere async Tasks
  über denselben Kanal, sticky addressing wäre eine Race-Condition-Quelle).
- **`SimulatedConnection::recv_line()` darf bei leerer Queue NIEMALS sofort
  einen Err zurückgeben**, sondern muss pending bleiben (`std::future::pending`).
  Sonst busy-loopt der Lab-Actor (`tokio::select!` sieht den Zweig ständig als
  ready). War schon einmal ein Bug, siehe Git-History.
- **Ein Actor pro Verbindung** (`Lab::spawn`), der die Connection exklusiv
  besitzt. Kein Mutex um die Connection selbst – das bildet die reale
  Hardware-Topologie ab (ein geteilter OptoBus).
- **`query()` gibt `Option<f64>` zurück, nicht `f64`** (JLab gibt bei Timeout
  0.0 zurück – das ist von einem validen Nullmesswert nicht unterscheidbar,
  bewusst vermieden).
- **DTOs für serde bleiben in `octlab-server`**, nicht in `octlab-protocol` –
  Protokoll-Ebene soll nicht von serde abhängen (Layer-Trennung).

## Build & Test

```bash
cargo test --workspace          # alle Crates (octlab-server zieht axum, dauert länger)
cargo test -p octlab-lab         # schneller Kernel-Test während der Entwicklung
cargo run -p octlab-server       # startet auf :3000, läuft OHNE Hardware (SimulatedConnection)
curl localhost:3000/health
```

Cross-Compile-Ziele (noch nicht in CI eingerichtet):
- `aarch64-unknown-linux-gnu` – Raspberry Pi, via `cross build --target ...`
- Tauri-Bundles: Windows/macOS/Linux via GitHub-Actions-Matrix
  (`windows-latest`, `macos-latest`, `ubuntu-latest`) – siehe
  `.github/workflows/ci.yml`, sobald angelegt. macOS-Signierung braucht einen
  echten Mac-Runner, nicht cross-compilebar von Linux aus.

## Nächste Schritte (Reihenfolge, nicht alles auf einmal)

1. `TcpConnection` in `octlab-transport` (roher Socket zum XPort, Port 10001)
2. Weitere Module in `octlab-devices`: Dcg, Div, AdaIo – Subkanal-Zuordnung
   IMMER gegen die tagesaktuelle Syntax-Tabelle auf www.ct-lab.de verifizieren,
   nicht blind aus den PDF-Artikeln von 2007 übernehmen (Firmware-Updates
   haben Subkanäle teils verschoben, siehe "Flashen der c't-Lab-Firmware.pdf").
3. Persistenz (SurrealDB embedded, `kv-rocksdb`) für Messreihen-Aufzeichnung –
   eigene Schicht, nicht in `octlab-lab` – Vorschlag: `octlab-recording`-Crate,
   die `lab.subscribe()` konsumiert und optional in SurrealDB schreibt.
4. `apps/web` (Leptos) – erst UI, wenn Backend-Kern stabil ist.
5. `apps/desktop` (Tauri) – bündelt `octlab-server` + `apps/web`.

## Für den Menschen im Projekt

Lernt gerade Rust, kommt aus Pascal/Business-Applications (ERP/CAQ/SQL/Atlassian).
Bei größeren Refactorings: Ownership-/Borrow-Entscheidungen kurz kommentieren,
nicht nur stillschweigend den Compiler-Fehler wegmachen – das ist der Teil, der
in Pascal keine Entsprechung hat und wo das Lernen stattfindet.
