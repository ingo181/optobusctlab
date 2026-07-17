# 0002 – Leptos-Frontend mit DIV-Gauge

Status: In Arbeit

> **Stand:** AK1–AK7 grün (`cargo test -p octlab-web` bzw.
> `cargo test -p octlab-server --test frontend_dist`). Der komplette Stack
> TcpConnection→Protokoll→Lab-Actor→WebSocket→Frontend ist zusätzlich
> end-to-end gegen einen Fake-XPort (Scratchpad-Skript, spricht das echte
> Draht-Protokoll `1:0?` → `#1:0=<wert>`) verifiziert: tickende,
> sich ändernde Werte auf `/ws`. AK8 (Live-Beweis am realen Gerät) steht
> noch aus - das XPort war beim Umsetzen nicht erreichbar (Anlage
> vermutlich ausgeschaltet). Sobald AK8 bestanden ist: Status auf
> `Umgesetzt` setzen.

## Kontext

Der komplette Durchstich Hardware→Server→Browser existiert bisher nur als
Wegwerf-Provisorium (statisches HTML mit Inline-JS in
`crates/octlab-server/static/index.html`, siehe CLAUDE.md "Nächste Schritte",
Schritt 3). Diese Spec ersetzt das Provisorium durch den ersten echten
Frontend-Baustein: ein `apps/web`-Crate (Leptos CSR, gebaut mit Trunk), das
die per WebSocket gepushten Messwerte hält und den DIV-Messwert (Adresse 1,
Subkanal 0) als Zeigerinstrument anzeigt – dieselbe App später unverändert im
Browser (Pi-Kiosk) und in der Tauri-WebView.

Referenz-Stack aus dem opnCAQ-Projekt: Leptos 0.8 CSR + Trunk,
`LocalResource` statt `Resource::new` für WASM-Futures, Thaw nicht verwenden
(0.4.x inkompatibel mit Leptos 0.8).

## Akzeptanzkriterien

### AK1: Letzter Wert pro Kanal gewinnt

Gegeben ein leerer Messwert-Zustand
Wenn nacheinander zwei Messwert-Nachrichten für denselben Kanal
(Adresse 1, Subkanal 0) mit den Werten 1.5 und 2.5 eintreffen
Dann enthält der Zustand für diesen Kanal genau den Wert 2.5

### AK2: Kanäle bleiben getrennt

Gegeben ein leerer Messwert-Zustand
Wenn je eine Messwert-Nachricht für (Adresse 1, Subkanal 0) und
(Adresse 2, Subkanal 3) eintrifft
Dann hält der Zustand beide Werte unter ihrem jeweiligen Kanal-Schlüssel

### AK3: Unlesbare Nachricht wird verworfen

Gegeben ein Messwert-Zustand mit einem vorhandenen Wert
Wenn eine Nachricht eintrifft, die kein gültiges Messwert-JSON ist
Dann bleibt der Zustand unverändert (kein Absturz, kein Datenverlust)

### AK4: Zeigerwinkel linear zwischen Skalenanfang und -ende

Gegeben ein Gauge mit Skala von 0.0 bis 10.0
Wenn der Messwert 0.0 / 5.0 / 10.0 beträgt
Dann steht der Zeiger auf dem Winkel des Skalenanfangs / genau mittig /
auf dem Winkel des Skalenendes

### AK5: Werte außerhalb der Skala schlagen nur bis zum Anschlag aus

Gegeben ein Gauge mit Skala von 0.0 bis 10.0
Wenn der Messwert unter 0.0 oder über 10.0 liegt (auch NaN als Sonderfall)
Dann bleibt der Zeiger am jeweiligen Skalen-Anschlag stehen (NaN: unterer
Anschlag), statt aus dem Instrument herauszudrehen

### AK6: Server liefert das gebaute Frontend aus

Gegeben ein Verzeichnis mit Trunk-Build-Ausgabe (mindestens `index.html`)
Wenn der Server mit diesem Verzeichnis als Frontend-Quelle startet und `/`
angefragt wird
Dann liefert die Antwort den Inhalt dieser `index.html` (Status 200)

### AK7: Fehlendes Frontend-Build erklärt sich selbst

Gegeben ein Frontend-Verzeichnis, das nicht existiert (Trunk-Build nie gelaufen)
Wenn `/` angefragt wird
Dann antwortet der Server mit 404 und einem Hinweistext, der `trunk build`
als Abhilfe nennt – statt eines kommentarlosen 404

### AK8: Live-Beweis am echten Gerät (manuell)

Gegeben der Server läuft mit `--connection tcp` gegen das reale c't-Lab und
serviert das gebaute Frontend
Wenn die Seite im Browser geöffnet ist
Dann bewegt sich der Gauge-Zeiger mit den echten DIV-Messwerten (Adresse 1,
Subkanal 0), und der Zahlenwert darunter tickt mit

## Explizit außerhalb des Scopes

- Subscription-/Sweep-Logik im Frontend (der provisorische 500ms-Poll
  `poll_div_provisional` im Server bleibt vorerst der Taktgeber, siehe
  CLAUDE.md)
- WebSocket-Reconnect nach Verbindungsabriss (Seite neu laden reicht für
  diese Ausbaustufe)
- Tailwind 4: kommt erst mit der ersten echten UI-Ausbaustufe (mehr als ein
  Instrument), nicht für ein einzelnes SVG-Gauge – Stack-Entscheidung dafür
  steht aber fest (opnCAQ-Referenz)
- Weitere Instrumente/Module, Konfigurierbarkeit der Gauge-Skala zur Laufzeit
- Umbau `apps/desktop` auf gebündeltes Frontend (bleibt WebView auf
  `http://localhost:3000`, zeigt damit automatisch das neue Frontend)
