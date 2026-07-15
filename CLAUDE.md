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

## Verifizierte Hardware-Fakten (realer Aufbau, kein Annahme aus 2007er-PDFs)

Am realen c't-Lab-Aufbau gemessen; bei Widerspruch zur Doku aus den
Original-Artikeln gilt das hier.

- **XPort** erreichbar unter `192.168.1.104:10001`, rohes TCP, "Accept
  Incoming: Yes" (Voraussetzung für `TcpConnection`, siehe "Nächste Schritte").
- **Serielle Einstellung am XPort: 38400/8/None/1** – musste von einem
  falschen Default (9600) korrigiert werden.
- **Vier Module per `*:IDN?` verifiziert:**
  - Adresse 0: ADA-IO, FW 1.742, bestückt mit DA12/AD16/IO32/LCD
  - Adresse 1: DIV, FW 3.10
  - Adresse 2: DCG, FW 2.92
  - Adresse 4: DDS, FW 3.71
  - Adresse 3: unbelegt – die Discovery-Semantik im ESDM-Modell (ein Modul
    antwortet oder eben nicht) bildet das ab, kein Sonderfall nötig.
- **Beispielantwort** auf `1:VAL 0?`: `#1:0=0.0022024` (DIV, offene Klemmen,
  Wert schwankt zwischen Messungen – ADC-Rauschen an offenen Klemmen, kein
  Fehler).
- **Zeilenende ist konsistent CR/LF (`\r\n`, Bytes `0d 0a`)** – per
  Byte-Level-Test (`xxd` auf die rohen TCP-Antwortbytes) verifiziert, keine
  Ausnahme über mehrere Kommandos/Module hinweg beobachtet.
- **Echo-Verhalten ist kommando-abhängig, nicht pauschal:** Ein
  **Broadcast**-Kommando (`*:IDN?`) erscheint selbst zuerst im
  Empfangsstream, bevor die eigentlichen Modul-Antworten folgen (reproduziert
  über mehrere Testläufe). Ein **adressiertes** Kommando (`1:IDN?`,
  `1:VAL 0?`) zeigt dagegen KEIN Echo – direkt nur die Antwort, ohne
  vorausgehende Zeilen. Die Handhabung dafür liegt bewusst generisch in
  `parse_message`/`Lab::dispatch` (jede Zeile ohne `#`-Präfix wird verworfen,
  Echo ist nur einer von mehreren möglichen Fällen ungültiger Eingabezeilen,
  keine eigene Sonderbehandlung nötig) – spezifiziert über
  `crates/octlab-lab/tests/features/malformed_input.feature`.
- **TCP-Fragmentierung tritt real auf, nicht nur synthetisch im Test:** Beim
  Hardwaretest von `*:IDN?` kam die Fünf-Zeilen-Antwort (Echo + vier Module)
  über drei separate `read()`-Aufrufe (97 + 36 + 46 Bytes) herein, davon
  einer mitten im Wort geschnitten (`"...2.92 [D"` / `"CG by CM/c't
  05/2010]..."`, mitten in "DCG"). `TcpConnection::recv_line()` (siehe
  `specs/0001-tcp-connection.md`, AK5) hat trotzdem korrekt rekonstruiert.
  Bestätigt: das ist kein theoretisches Edge-Case-Szenario, sondern normales
  Verhalten dieser Hardware/dieses XPorts.

## Dev Container (Podman)

Runtime ist **Podman**, nicht Docker (Linux nativ + Windows/WSL rootless).
Dateien unter `.devcontainer/` (`Containerfile`, `devcontainer.json`).

```bash
podman build -t optobusctlab-dev -f .devcontainer/Containerfile .
podman run --rm -it \
  -v "$(pwd)":/workspace -w /workspace \
  -v optobusctlab-cargo-target:/cargo-target \
  optobusctlab-dev bash
# im Container:
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check licenses
esdm lint domain.esdm.yaml
```

- **Ein Quellbaum, aber getrennte `target/`-Verzeichnisse:** Der Projektordner
  wird per Bind-Mount nach `/workspace` gemountet (`workspaceMount` in
  `devcontainer.json`, bzw. `-v "$(pwd)":/workspace` beim manuellen
  `podman run`) – Container (Tests, Server, esdm, cargo-deny) und Host
  (Tauri nativ mit GUI) arbeiten also auf denselben Dateien, kein Duplikat,
  kein Sync. Der Cargo-Build-Cache wird davon aber bewusst ausgenommen: Im
  Container ist `CARGO_TARGET_DIR=/cargo-target` gesetzt (`ENV` im
  Containerfile), gestützt durch ein eigenes Podman-Volume
  (`optobusctlab-cargo-target`, in `devcontainer.json` unter `mounts`, bzw.
  `-v optobusctlab-cargo-target:/cargo-target` beim manuellen Aufruf). Würde
  man stattdessen `/workspace/target` von Container und Host teilen, würde
  ein Cargo-Build im jeweils anderen Kontext (andere `rustc`-Version, andere
  Host-Triple) den kompletten Cache invalidieren – ständige Full-Rebuilds auf
  beiden Seiten. Zusätzlich hätte ein Bind-Mount für `target/` bei rootless
  Podman UID-Mapping-Probleme zur Folge (im Container erzeugte Dateien
  gehören auf dem Host einem anderen User); ein von Podman verwaltetes
  named volume vermeidet das, weil Podman die Ownership beim Anlegen selbst
  passend setzt. Verifiziert: Eine im Container geänderte Datei ist sofort
  auf dem Host sichtbar (Bind-Mount); ein `cargo build` im Container legt
  ausschließlich unter `/cargo-target` (= das Volume) ab und lässt ein
  vorhandenes Host-`target/` unangetastet, ein anschließender Host-Build
  kompiliert dort weiter inkrementell statt komplett neu; nach einem
  Container-Build gehören alle Dateien unter `/workspace` auf dem Host
  weiterhin dem ursprünglichen User (keine Permission-Probleme), weil unter
  `target/` nichts mehr in den Bind-Mount geschrieben wird.
- **Normales rootless Networking reicht, kein `--network=host` nötig.**
  `devcontainer.json` setzt bewusst kein `runArgs: ["--network=host"]` –
  der Container läuft mit Podman 6s rootless-Default (`pasta`), Port 3000
  ist über `forwardPorts` deklariert, und die LAN-Erreichbarkeit des XPort
  (`192.168.1.104:10001`) ergibt sich automatisch aus `pasta`s NAT/Routing
  (kein Host-Routing-Trick nötig). Verifiziert über die echte
  `@devcontainers/cli` (nicht nur manuelles `podman run`): `NetworkMode`
  des gestarteten Containers ist `pasta`, `*:IDN?` liefert alle vier Module
  (Adressen 0/1/2/4, siehe "Verifizierte Hardware-Fakten" oben), und
  `cargo test --workspace` läuft grün darin.
- **Troubleshooting: `invalid default_rootless_network_cmd option
  "slirp4netns"` beim Container-Start.** Kommt auf Podman-≥6-Systemen vor,
  wenn `~/.config/containers/containers.conf` noch einen Eintrag
  `default_rootless_network_cmd = "slirp4netns"` aus Podman-5-Zeiten hat –
  Podman 6 akzeptiert dort laut `man containers.conf` nur noch `"pasta"`
  (der ohnehin neue Default). Kein Repo-Problem, kein Grund für
  `--network=host` als Workaround: einfach die veraltete Zeile aus
  `containers.conf` entfernen (oder auf `"pasta"` ändern) und `pasta`
  installiert lassen (Paket `passt`). Auf dieser Maschine war genau das
  die Ursache; nach dem Entfernen der Zeile lief alles oben Beschriebene
  ohne jeden Host-networking-Flag.
- **`esdm`-Binary wird im Container frisch heruntergeladen**, nicht aus dem
  lokal vendorten `/esdm` (das liegt in `.gitignore`, ist Version 0.12.0 und
  auf esdm.io nicht mehr mit Prüfsumme verifizierbar – die Website
  veröffentlicht SHA256 nur für die jeweils aktuelle Version, aktuell 0.14.0,
  keine historischen Prüfsummen). Der Containerfile-Download zieht
  `esdm-linux-amd64` Version 0.14.0 von
  `https://esdm.s3.fr-par.scw.cloud/0.14.0/esdm-linux-amd64` und verifiziert
  gegen die auf esdm.io/getting-started/installing-esdm/ veröffentlichte
  SHA256 (`c0a786972300f6f7e71e645f009b8e7b8b7967c8837daf9e51f968e756e1716e`)
  per `sha256sum -c`, bevor die Datei ausführbar gemacht wird.
- **Tauri bewusst NICHT im Container**: keine `webkit2gtk`/`libsoup`-Pakete
  im Containerfile. Das Desktop-Bundle wird nativ auf dem jeweiligen Host-OS
  gebaut (siehe die auskommentierte `tauri-build`-Matrix in
  `.github/workflows/ci.yml`), nicht im Dev-Container.
- **VS-Code-Extensions vordeklariert** in `devcontainer.json`
  (`customizations.vscode.extensions`): `rust-lang.rust-analyzer`,
  `tamasfe.even-better-toml`, `stevejpurves.cucumber` (Gherkin-Syntax für
  die `.feature`-Dateien unter `crates/octlab-lab/tests/features/`).
- **Cross-Compiling (Pi/aarch64) bleibt außerhalb des Dev-Containers**, siehe
  unten – das ist ein CI-Konzern, kein Dev-Loop-Konzern.
- **`cargo-deny` ist auf `0.18.3` gepinnt** (Containerfile), nicht die
  neueste Version. `cargo-deny` ab 0.19 verlangt rustc ≥1.88, das
  Basisimage `rust:1.85-bookworm` bringt aber 1.85.1 mit; 0.18.3 ist die
  letzte Version, die damit noch baut. Beim nächsten Bump des Basisimages
  auf rustc ≥1.88 kann der Pin entfallen.

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

1. ~~`TcpConnection` in `octlab-transport` (roher Socket zum XPort, Port
   10001)~~ – **erledigt**, siehe `specs/0001-tcp-connection.md` (Status
   "Umgesetzt") und "Verifizierte Hardware-Fakten" oben.
2. Anbindung an `octlab-lab`: Der Actor (`Lab::spawn`) nimmt aktuell
   irgendeinen `Box<dyn BoardConnection>` entgegen, aber es gibt noch keinen
   Weg, im laufenden Betrieb (Server-Start, UI) zwischen `SimulatedConnection`
   (Default, hardware-frei) und `TcpConnection` (echte Anlage) zu wählen –
   das muss noch verdrahtet werden (z.B. Konfiguration/Env-Var in
   `octlab-server`).
3. Weitere Module in `octlab-devices`: Dcg, Div, AdaIo – Subkanal-Zuordnung
   IMMER gegen die tagesaktuelle Syntax-Tabelle auf www.ct-lab.de verifizieren,
   nicht blind aus den PDF-Artikeln von 2007 übernehmen (Firmware-Updates
   haben Subkanäle teils verschoben, siehe "Flashen der c't-Lab-Firmware.pdf").
4. Persistenz (SurrealDB embedded, `kv-rocksdb`) für Messreihen-Aufzeichnung –
   eigene Schicht, nicht in `octlab-lab` – Vorschlag: `octlab-recording`-Crate,
   die `lab.subscribe()` konsumiert und optional in SurrealDB schreibt.
5. `apps/web` (Leptos) – erst UI, wenn Backend-Kern stabil ist.
6. `apps/desktop` (Tauri) – bündelt `octlab-server` + `apps/web`.

## Backlog (kein aktiver Schritt, nur vorgemerkt)

- **Mnemonic-Syntax (`VAL`, `FRQ`, `IDN`, ...) statt reiner Subkanal-Nummern
  in `Command::to_wire()`** – bewusst NICHT umgesetzt (Entscheidung bei
  Spec 0001, s. `specs/0001-tcp-connection.md`): Antworten kommen immer
  numerisch zurück, `ChannelKey` muss also so oder so die numerische
  Subkanal-Nummer als kanonische Identität führen. Mnemonics beim Senden
  lösen das Firmware-Drift-Problem an der jetzigen Architektur NICHT (ein
  verschobener Subkanal bricht die Pending-Map-Korrelation trotzdem), sie
  wären nur kosmetisch. Erst relevant, wenn Firmware-Drift real zuschlägt
  UND wir bereit sind, eine dynamische Syntax-Auflösung zur Laufzeit zu
  bauen (Subkanal-Zuordnung von der Hardware selbst abfragen statt statisch
  in `octlab-devices` zu hardcoden) – dann eigene Spec, kein Nebenbei-Umbau.

## Für den Menschen im Projekt

Lernt gerade Rust, kommt aus Pascal/Business-Applications (ERP/CAQ/SQL/Atlassian).
Bei größeren Refactorings: Ownership-/Borrow-Entscheidungen kurz kommentieren,
nicht nur stillschweigend den Compiler-Fehler wegmachen – das ist der Teil, der
in Pascal keine Entsprechung hat und wo das Lernen stattfindet.
