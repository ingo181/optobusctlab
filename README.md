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

## Dev Container

`.devcontainer/` (Podman, nicht Docker) enthält den kompletten Rust-Toolchain
mit `cargo-deny` und `esdm`-CLI für den Backend-Kern (die fünf `crates/`).
Details, Build-/Run-Kommandos und bekannte Podman-Netzwerk-Stolpersteine
stehen in [`CLAUDE.md`](./CLAUDE.md#dev-container-podman).

**Tauri läuft bewusst NICHT im Dev-Container.** Das Desktop-Bundle
(`apps/desktop`) braucht `webkit2gtk`/native GUI-Libs und wird nativ auf dem
jeweiligen Host-OS gebaut (Windows/macOS/Linux je eigener CI-Runner) – der
Dev-Container bleibt schlank und bezieht sich nur auf den Backend-Kern.

## Desktop-Bundle (Tauri)

`apps/desktop` bündelt Server + Frontend zu einem installierbaren Artefakt.
Aktuell nur Linux/AppImage (Windows/macOS folgen über eine CI-Matrix), Details
und Build-Kette in [`CLAUDE.md`](./CLAUDE.md).

**Bekannte Einschränkung:** AppImages brauchen zum Start FUSE
(`libfuse2`/`libfuse3`). Viele aktuelle Distributionen bringen `libfuse2`
nicht mehr standardmäßig mit – ohne FUSE startet das `.AppImage` nicht
(`dlopen(): error loading libfuse.so.2` o.ä.). Workaround ohne
Installation von Zusatzpaketen:

```bash
./octlab-desktop_<version>_amd64.AppImage --appimage-extract-and-run
```

Das entpackt das Image in ein Temp-Verzeichnis und führt es von dort direkt
aus, ganz ohne FUSE-Mount.

## Lizenz

[MIT](./LICENSE). Basiert auf öffentlich dokumentiertem Protokoll-Wissen aus
der c't-Lab-Artikelserie (Heise) sowie der JLab- und CtLab-Library-Doku;
es wird kein Code aus diesen Vorgänger-Projekten übernommen.
