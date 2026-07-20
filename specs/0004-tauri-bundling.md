# 0004 – Tauri-Bundling mit eingebettetem Frontend

Status: Umgesetzt

> **Nachweis:** AK1/AK2 grün (`cargo test -p octlab-server --features
> embed-frontend` existierte im ersten Anlauf, siehe unten - nach dem
> Architekturwechsel ersetzt durch `cargo test -p octlab-server --test
> frontend_dist`, unverändert grün unter `cargo test --workspace`). AK4
> (Hook regeneriert `apps/web/dist` aus einem sauberen Zustand: `dist`
> manuell entfernt, `cargo tauri build --bundles appimage` lief trotzdem
> durch, `trunk build`-Log als erste Zeile bestätigt) und AK5 (`.AppImage`
> lag danach unter `target/release/bundle/appimage/`) live gebaut,
> 2026-07-20. AK6 (Live-Beweis) bestanden: Artefakt nach `/tmp` kopiert,
> von dort mit `OCTLAB_CONNECTION=tcp OCTLAB_ADDR=127.0.0.1:15001` gegen
> `fake_xport` gestartet - `curl localhost:3000/health` → `{"status":"ok"}`,
> `curl localhost:3000/` → enthält `<title>optobusctlab</title>` aus dem
> eingebetteten Frontend, manueller WebSocket-Handshake auf `/ws` lieferte
> eine echte, von `fake_xport` simulierte DIV-Messung
> (`{"address":1,"subchannel":0,"value":0.0025349,"status_text":null}`),
> `POST /api/channel/4/0` lief bis zum erwarteten Timeout (fake_xport
> quittiert keine Set-Kommandos, siehe CLAUDE.md) statt zu crashen. Native
> Fensterinstanz lief sichtbar auf dem Desktop des Betreibers (Wayland-
> Session bestätigt), Sichtprüfung durch den Betreiber selbst (kein
> Screenshot-Tool in der Ausführungsumgebung verfügbar).

## Kontext

Bisher war `apps/desktop` nur ein Provisorium: `cargo run -p octlab-desktop`
startet einen eingebetteten Server, der das Frontend zur Laufzeit von
`apps/web/dist` auf der Platte liest (`OCTLAB_FRONTEND_DIST`). Das
funktioniert nur, solange Repo und `dist`-Ordner nebeneinander liegen – kein
installierbares Artefakt für einen fremden Rechner ohne Repo.

Diese Spec macht daraus ein echtes Bundle (vorerst nur Linux/AppImage,
Windows/macOS bleiben CI-Zukunft). Vorab abgestimmte Entscheidungen:

- **Einbettung übers eigene Binary (`rust-embed`), nicht über Tauris
  `frontendDist`-Mechanismus.** Die WebView bleibt bei
  `http://localhost:3000` wie im bisherigen Provisorium – kein
  Origin-Wechsel, keine Anpassung der relativen `/api`/`/ws`-URLs im
  Frontend nötig.
- **Build-Reihenfolge über Tauris `build.beforeBuildCommand`**, nicht
  `build.rs` oder ein separates Makefile. Greift nur bei einem echten
  `tauri build`, lässt `cargo test --workspace`/`cargo build -p
  octlab-server` und den Dev-Container (kein trunk/wasm32 dort) unberührt.
  **Wichtige, erst beim Ausprobieren gefundene Tatsache:** Der Working
  Directory für `beforeBuildCommand` ist NICHT der Ordner, der
  `tauri.conf.json` enthält (`apps/desktop`), sondern dessen
  ELTERNVERZEICHNIS (`apps/`) – verifiziert durch einen Diagnose-Lauf
  (`"beforeBuildCommand": "pwd && ls .."`). Der Hook lautet deshalb
  `"cd web && trunk build"` (relativ zu `apps/`), NICHT `"cd ../web && ..."`
  (das schlägt fehl, weil es zwei Ebenen zu weit nach oben zeigt).
- **Bundle-Ziel AppImage**, nicht `.deb` – auf EndeavourOS (Arch-basiert,
  kein `dpkg`) der einzige Weg, das Artefakt ohne Umwege lokal zu
  installieren/zu starten. Braucht `bundle.icon` in `tauri.conf.json`
  (`icons/icon.png`, bereits vorhanden) – ohne den Eintrag bricht das
  AppImage-Bundling mit "couldn't find a square icon" ab.
- **Konfiguration weiterhin über Env-Vars** (`OCTLAB_CONNECTION`,
  `OCTLAB_ADDR`, bereits vorhanden) – ein Settings-UI ist eine spätere,
  eigene Einheit.

**Architektur-Kurskorrektur während der Umsetzung (wichtig für künftige
ähnliche Fälle):** Der ursprüngliche Plan sah ein Cargo-Feature
`embed-frontend` IN `octlab-server` vor (per `#[cfg]` zwischen
Platten-`ServeDir` und eingebettetem `rust-embed` umschaltbar), das
`apps/desktop` aktiviert. Das wurde gebaut, kompilierte einwandfrei
einzeln - brach aber `cargo test --workspace` auf eine nicht offensichtliche
Art: Cargo unifiziert Features EINER gemeinsamen Abhängigkeit über ALLE
Workspace-Mitglieder hinweg, die im selben Build-Graph landen. Da
`apps/desktop` `octlab-server` mit `embed-frontend` einbindet, wurde
`octlab-server` SELBST beim `cargo test --workspace`-Lauf mit aktivem
Feature kompiliert - dessen eigene `ServeDir`-Tests
(`tests/frontend_dist.rs`) liefen dadurch unbemerkt gegen das eingebettete
Frontend statt gegen ein Test-Temp-Verzeichnis und schlugen fehl. Ursache:
das Feature war NICHT rein additiv (es ERSETZTE bestehendes Verhalten
statt nur neues hinzuzufügen) - genau das Muster, bei dem Cargos
Feature-Unification zuschlägt. Behoben durch Verschieben der gesamten
`rust-embed`-Logik aus `octlab-server` heraus in `apps/desktop` selbst:
`octlab-server` exportiert jetzt zusätzlich `build_app_without_frontend`
(Health/WS/Set-Channel-Routen ohne Frontend-Fallback), `apps/desktop` hängt
seinen eigenen `rust-embed`-Fallback-Handler an. `rust-embed` ist dadurch
ausschließlich eine `apps/desktop`-Abhängigkeit, taucht in `octlab-server`s
eigenem Dependency-Baum gar nicht mehr auf - das Problem ist strukturell
ausgeschlossen, nicht nur umgangen. Nebeneffekt: der ursprünglich erhoffte
Zusatznutzen "löst nebenbei auch das Pi-Single-Binary-Problem" entfällt
damit für `octlab-server` selbst; ein späteres Pi-Bundling bräuchte
denselben Kniff (eigener Fallback-Handler in einem eigenen Aufrufer-Crate),
kein Selbstläufer mehr über ein Feature. Das ist ein akzeptabler Trade-off
für einen echten Korrektheitsgewinn, aber bewusst hier festgehalten, damit
es nicht als Regression missverstanden wird.

Ebenfalls erst beim Bundling entdeckt: `linuxdeploy`s eingebettetes `strip`
(altes bundled Binutils) kann mit `.relr.dyn`-Sektionen nichts anfangen, die
der aktuelle Arch/EndeavourOS-Systemtoolchain (neuere `glibc`/`binutils` mit
"packed relative relocations") standardmäßig erzeugt - Bundling schlug mit
zahlreichen "Strip call failed: unknown type [0x13] section `.relr.dyn`"
fehl. Kein Bug in diesem Projekt, sondern eine bekannte
Ökosystem-Inkompatibilität zwischen AppImage-Tooling und Rolling-Release-
Distributionen. Workaround: Umgebungsvariable `NO_STRIP=1` beim Aufruf von
`cargo tauri build` setzen (linuxdeploy liest sie und überspringt das
Stripping). Nicht im Repo verankert (kein `.cargo/config.toml`-Eintrag o.ä.),
weil es eine Eigenschaft DIESER Bau-Maschine ist, keine des Projekts -
bei Bedarf (z.B. CI-Actions-Matrix später) dort separat setzen.

Lizenz-Vorprüfung: `rust-embed` (inkl. `mime-guess`-Feature) wurde vor der
Implementierung gegen `cargo deny check licenses` geprüft, Ergebnis
`licenses ok` ohne Änderung an `deny.toml` - auch nach dem Umzug von
`octlab-server` nach `apps/desktop` erneut bestätigt. Das AppImage-Bundling
selbst (`tauri-cli`/`linuxdeploy`) liegt außerhalb des
Workspace-Dependency-Graphen und damit außerhalb der Reichweite von
`cargo deny`.

## Akzeptanzkriterien

### AK1: Ohne Frontend-Einbettung bleibt der bisherige Dev-Loop unverändert

Gegeben `octlab-server` (Standardbauform, keine Sonderkonfiguration)
Wenn der Server startet und `apps/web/dist` zur Laufzeit vorhanden ist
Dann wird das Frontend wie bisher von der Platte serviert
(`OCTLAB_FRONTEND_DIST`, Default `apps/web/dist`), keine Verhaltensänderung
gegenüber dem Stand vor dieser Spec

### AK2: `apps/desktop` ist ohne `dist`-Ordner auf der Zielmaschine lauffähig

Gegeben `apps/desktop` wurde gebaut (Build-Zeitpunkt: `apps/web/dist` war
vorhanden und gefüllt)
Wenn das Binary/AppImage an einem Ort gestartet wird, an dem kein
`apps/web/dist` existiert (auch nicht relativ auffindbar)
Dann liefert `/` trotzdem den Frontend-Inhalt aus dem eingebetteten Bundle,
nicht den "trunk build"-Hinweis

### AK3: `apps/desktop` bringt sein eigenes eingebettetes Frontend mit

Gegeben `apps/desktop`s Quellcode
Wenn der Server-Router zusammengebaut wird
Dann geschieht das über `octlab_server::build_app_without_frontend` plus
einen `apps/desktop`-eigenen `rust-embed`-Fallback-Handler - `octlab-server`
selbst kennt keine Einbettung

### AK4: `tauri build` baut das Frontend automatisch mit

Gegeben ein sauberer Checkout ohne vorhandenes `apps/web/dist`
Wenn `cargo tauri build` (aus `apps/desktop`) läuft
Dann läuft vorher automatisch `trunk build` (über `build.beforeBuildCommand`
in `tauri.conf.json`, Working Directory `apps/`), ohne dass der Bediener
das manuell anstoßen muss

### AK5: Bundle-Ziel AppImage wird erzeugt

Gegeben `cargo tauri build --bundles appimage` auf Linux
Wenn der Build durchläuft
Dann liegt danach eine `.AppImage`-Datei im Tauri-Bundle-Ausgabeverzeichnis

### AK6: Live-Beweis – das Artefakt ist selbsttragend (manuell)

Gegeben die gebaute `.AppImage`-Datei
Wenn sie an einen Ort AUSSERHALB des Repos kopiert (z.B. `/tmp`) und von dort
gestartet wird (nicht `cargo run`, nicht aus dem Repo-Arbeitsverzeichnis)
Dann öffnet sich das native Fenster, zeigt Gauge und Frequenz-Bedienfeld,
und beides funktioniert gegen `fake_xport` oder die reale Anlage – ohne dass
irgendein Pfad im Repo existieren muss

## Explizit außerhalb des Scopes

- Windows-/macOS-Bundles (CI-Actions-Matrix, spätere Einheit)
- `.deb`/`.rpm` (später für CI/Distribution relevant, nicht für den lokalen
  Test auf EndeavourOS)
- Settings-UI zur Laufzeit-Konfiguration (bleibt Env-Var-basiert)
- Frontend-Einbettung für `apps/web` selbst im Browser-Betrieb (Trunk-
  Dev-Server/`trunk build` + `ServeDir` bleiben wie bisher, das betrifft nur
  den Desktop-/Bundle-Pfad)
- Cross-Compile für Raspberry Pi/aarch64 und ein eingebettetes Frontend für
  den eigenständigen `octlab-server`-Prozess selbst (eigener, späterer
  Schritt - siehe Kontext: die "löst zwei Probleme auf einmal"-Hoffnung aus
  der Vorab-Abstimmung hat sich als nicht tragfähig erwiesen, siehe
  Architektur-Kurskorrektur oben)
- Automatisiertes Umgehen der `linuxdeploy`/`strip`-Inkompatibilität (z.B.
  fest im Repo verankertes `NO_STRIP=1`) - bleibt eine Bau-Maschinen-Notiz

## Offene Fragen

Keine.
