# optobusctlab – Projektkontext für Claude Code

**Repo-/Projektname: `optobusctlab`** (GitHub, README, Produktname).
**Interner Crate-Präfix: `octlab`** (kurz für Opto-Bus-ctlab, aus Tipp-Ergonomie
in `use`-Statements – nicht mit dem Repo-Namen verwechseln, das ist bewusst
kürzer). Falls das Projekt wachsen sollte, ist eine spätere Umbenennung auf
den vollen `optobusctlab-*`-Präfix ein reines Suchen/Ersetzen, kein
strukturelles Problem.

## Workflow für (nahezu) autonome Sessions: Spec- und Test-Driven

Jede Aufgabe läuft nach diesem Zyklus – Ziel ist, dass eine Claude-Code-Session
in RustRover mehrere Schritte selbständig durchläuft, ohne nach jedem
Zwischenschritt auf Bestätigung zu warten. Spec und Tests sind das
Sicherheitsnetz, das diese Autonomie erst verantwortbar macht.

1. **Spec zuerst.** Für jede neue Fähigkeit (neues Modul, neuer Endpoint, neue
   Transport-Implementierung) liegt eine Datei unter `specs/NNNN-kurzer-name.md`
   (fortlaufend nummeriert, Vorlage: `specs/TEMPLATE.md`). Eine Spec beschreibt
   WAS und WARUM, nicht WIE – keine Rust-Typen, keine Implementierungsdetails.
   Akzeptanzkriterien im Given/When/Then-Format.
2. **Tests aus der Spec ableiten.** Jedes Akzeptanzkriterium → mindestens ein
   Test. Test schreiben, `cargo test` laufen lassen, ROT bestätigen (explizit
   sehen, dass er fehlschlägt – ein Test, der nie rot war, hat nichts bewiesen).
3. **Minimal implementieren, bis GRÜN.** Nur so viel Code wie nötig für den
   aktuellen Test. Kein Vorgriff auf spätere Specs.
4. **Refactoring**, Tests bleiben grün (`cargo test` nach jedem Schritt).
5. **Definition of Done** für eine Spec:
   - Alle Akzeptanzkriterien haben einen grünen Test
   - `cargo test --workspace` komplett grün
   - `cargo clippy --workspace -- -D warnings` ohne Fehler
   - Spec-Datei-Status auf `Umgesetzt` gesetzt
   - `CLAUDE.md` aktualisiert, falls sich eine dokumentierte Design-Entscheidung
     geändert hat
6. **Nicht spekulativ vorbauen** (YAGNI) – kein Code für Specs, die noch nicht
   geschrieben sind, auch nicht "weil's sich anbietet".

Rückfragen an den Menschen sind trotzdem Pflicht, wenn:
- eine Spec mehrdeutig ist (lieber fragen als raten)
- eine Design-Entscheidung aus diesem Dokument verletzt werden müsste
- eine neue externe Dependency nötig wird, die noch nicht im Workspace ist

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

## Spezifikation: ESDM + Cucumber (Harness/Spec-Trennung)

Zwei Spezifikationsebenen, bewusst getrennt, kein Duplikat:

```
domain.esdm.yaml                      # WAS: Domain-Modell als ESDM-YAML (nicht
module-control/                       # ausführbar, nur gegen schemas/ gelintet:
  bounded-context.esdm.yaml           # `esdm lint`).
  module.esdm.yaml                    #   Aggregate `module` + seine Commands/Events.
  module.feature.esdm.yaml            #   Given-When-Then-Spezifikation des
                                       #   Aggregats (ESDM-GWT-Extension) - ebenfalls
                                       #   nur gelintet, nicht ausgeführt.
  actors.esdm.yaml                    #   Actors operator (human) / bus-receiver (system).

crates/octlab-lab/tests/
  features/*.feature                  # Given-When-Then-Specs (Gherkin), lesbar auch
                                       # ohne Rust-Kenntnisse, laufen als echte
                                       # `cargo test` gegen den Lab-Actor.
  cucumber.rs                         # Step-Definitionen dazu.
```

Aktueller Modellstand: Domain `optobusctlab` → Bounded Context
`module-control`, Aggregate `module` (identifiziert über `address`),
Commands `record-identification`/`record-status`/`record-channel-value`/
`set-channel-value`, Events `identified`/`status-received`/
`channel-value-received`/`channel-value-set-requested`. Ein Read-Model für
Verbindungsstatus/verbundene Module und ein Process-Manager für Sweep-/
Recording-Workflows sind im Modell bewusst noch NICHT angelegt (siehe
"Nächste Schritte" unten) - erst modellieren, wenn sie fachlich dran sind.

Verhältnis ESDM-GWT ↔ Gherkin/Cucumber: unterschiedliche Flughöhe. ESDM-GWT
(`module.feature.esdm.yaml`) beschreibt den Aggregat-Vertrag in reinen
Domänenbegriffen (Commands/Events, kein Draht-Format, kein Timeout) und wird
nur gelintet. Die Cucumber-Features testen die tatsächliche
`octlab-lab`-Implementierung (Draht-Strings, Lab-Actor, Timeout-Verhalten,
Broadcast) und laufen als echter Code. Das 500ms-Query-Timeout ist deshalb
bewusst nur in den Cucumber-Features modelliert, nicht im ESDM-Modell - es
ist keine Aggregat-Tatsache, sondern eine technische Eigenschaft der
Lab-Actor-API (dort gibt es auf Aggregat-Ebene ohnehin keinen Query-Command).

**Status Cucumber-Tests:** grün (`cargo test -p octlab-lab --test cucumber`,
2 Features, 3 Szenarien, 11 Steps). `cucumber` 0.21 verlangt
`#[derive(cucumber::World)]` statt eines von Hand geschriebenen
`impl World for LabWorld` (das ältere `#[derive(WorldInit)]`-Muster wurde
in 0.21 ersetzt) - der `LabWorld`-Struct trägt das Derive jetzt.

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

## Was gehört NICHT ins Repo

Das Repo ist public und MIT-lizenziert – Drittmaterial mit unklarer oder
inkompatibler Lizenz darf nicht rein, auch nicht versehentlich über einen
Commit, der "eigentlich" etwas anderes bringen sollte.

- **`esdm`-CLI-Binary** (liegt lokal im Repo-Root, `/esdm` in `.gitignore`).
  Lizenz laut Schema-Header ("Free to use and redistribute as-is.
  Modification is not permitted") ist NICHT MIT-kompatibel. War schon einmal
  versehentlich mitcommittet (`d325681`), rückwirkend bereinigt (`be37e16`,
  vor dem Push). Die `*.esdm.yaml`-Modelldateien und `schemas/` sind davon
  NICHT betroffen – die gehören sehr wohl ins Repo.
- **IDE-/Editor-Config** (`.idea/`, `.vscode/`, beide in `.gitignore`). Rein
  lokale Werkzeug-Config, keine Projektinformation. War ebenfalls schon
  einmal versehentlich getrackt (Initial-Commit), bereinigt in `6723c22`.
- **Referenzmaterial mit unklarer Lizenz**: c't-PDFs/Heise-Copyright-Material
  (z.B. "Flashen der c't-Lab-Firmware.pdf"), JLab-Doku/JARs, `ctlab.py`.
  Bleibt lokale Referenz auf der eigenen Maschine, NIE committen.

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
