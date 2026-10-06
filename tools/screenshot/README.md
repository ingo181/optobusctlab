# Headless-Screenshots des Frontends

Hilfsmittel für die Screenshot-Nachweise der Layout-Specs (zuerst
`specs/0006-touch-layout-800x480.md`): ein PNG des Frontends in einer
festen Fenstergröße, ohne echte Anlage, gegen den Simulator `fake_xport`.

## Aufruf

Vom Repo-Root:

```bash
tools/screenshot/shot.sh                      # 800x480, alle Module antworten
tools/screenshot/shot.sh --mute 2             # DCG stumm -> Übersicht zeigt "veraltet"
tools/screenshot/shot.sh --size 1000x1300 --out /tmp/gross.png
tools/screenshot/shot.sh --help               # alle Optionen
```

Ergebnis: `target/screenshots/frontend-<B>x<H>.png` (Default). `target/`
ist in `.gitignore` - Screenshots landen damit nie versehentlich im Repo.
Am Ende gibt das Skript zusätzlich die Poll-Statistik aus
(`GET /api/poll`).

## Was das Skript tut

1. Baut `fake_xport`, `octlab-server` und das Frontend (`trunk build`);
   `--no-build` überspringt das.
2. Startet `fake_xport` (optional mit `--mute`), `octlab-server --connection
   tcp` dagegen und den Hilfsserver (`helper_server.py`), merkt sich jede
   PID.
3. Wartet, bis `/health` antwortet, plus `--settle-ms`, damit der erste
   Poll-Zyklus durch ist (stumme Module laufen erst nach 500 ms je Kanal
   ins Timeout).
4. Lässt Headless-Firefox die Hilfsseite `frame.html` fotografieren: Sie
   bettet die App in ein iframe genau der gewünschten Größe ein. Was nicht
   hineinpasst, ist abgeschnitten - der Screenshot zeigt also genau das,
   was ohne Scrollen sichtbar ist.
5. Beendet beim Verlassen (auch bei Fehler oder Strg+C) genau die selbst
   gestarteten Prozesse über ihre PIDs und löscht das temporäre
   Firefox-Profil.

**Warum die Hilfsseite?** `firefox --screenshot` löst beim `load`-Ereignis
aus. Die WASM-App ist dann noch nicht gemountet, der Screenshot wäre leer.
Die Hilfsseite lädt deshalb zusätzlich ein Bild, das der Hilfsserver erst
nach `--wait-ms` (Default 8000) ausliefert - so lange wartet das
`load`-Ereignis, und die App hat Zeit, WASM zu laden, `/ws` zu verbinden
und die ersten Werte anzuzeigen.

## Voraussetzungen

- **Firefox** mit Headless-Modus (`firefox --headless`, ab Firefox 56 an
  Bord). Läuft mit eigenem Wegwerf-Profil (`--no-remote --profile`), ein
  geöffnetes Firefox des Nutzers bleibt unberührt.
- **python3** (nur Standardbibliothek, für den Hilfsserver), **curl**,
  **cargo**, für den Build zusätzlich **trunk** und das Rust-Target
  `wasm32-unknown-unknown` (siehe CLAUDE.md, "Frontend-Dev-Workflow").
- Freie Ports: 3000 (`octlab-server`, fest), 15077 (`fake_xport`,
  `FAKE_PORT`) und 18080 (Hilfsserver, `HELPER_PORT`). Das Skript bricht ab,
  wenn einer belegt ist - z.B. weil schon ein `octlab-server` läuft.
- Nicht im Dev-Container gedacht (dort gibt es kein Firefox und kein trunk).

## Was geprüft wird - und was nicht

Das Skript prüft **nichts automatisch**, es erzeugt den Screenshot. Die
Bewertung (z.B. "alle 12 Zeilen ohne Scrollen sichtbar", "Touch-Ziele etwa
44 px") erfolgt durch Ansehen des PNG gegen die Akzeptanzkriterien der
jeweiligen Spec. Die Werte stammen aus `fake_xport` und sind ERFUNDEN,
Aussagen über die echte Anlage oder das echte Touch-Display liefert der
Screenshot nicht (dafür gibt es die Messpunkte der Specs).
