# 0003 – DDS-Frequenz aus der UI setzen

Status: Umgesetzt

> **Nachweis:** AK1–AK3 grün (`cargo test -p octlab-lab --test cucumber`,
> Feature `set_channel_value.feature`), AK4/AK5 grün (`cargo test -p
> octlab-server --test set_channel`), AK6–AK8 grün (`cargo test -p
> octlab-web`, Modul `frequency`). AK9 am realen Gerät (2026-07-19,
> Endpoint-Ebene, rohe Antworten):
> `POST /api/channel/4/0 {"value":2500}` → HTTP 200
> `{"ack":"OK","value":2500.0}`; `{"value":2000000}` → HTTP 422
> `{"ack":"PARERR","value":999999.8}` (Klemmung, wie spezifiziert);
> `{"value":1000}` → HTTP 200 (Restore auf den vorherigen Wert).
> Sichtprüfung des Betreibers im Browser bestanden: 998 Hz im Bedienfeld
> gesetzt, bestätigte Frequenz aus dem Rücklesen angezeigt (Screenshot
> beim Betreiber). Anlage danach auf 1000.0 Hz zurückgestellt.

## Kontext

Bisher liest die Anwendung nur (DIV-Messwert per Poll → WebSocket → Gauge).
Diese Spec bringt den ersten **schreibenden** Hardware-Zugriff: der Bediener
gibt im Frontend eine Frequenz ein, das DDS-Modul (Adresse 4) stellt sie.
Damit entsteht der erste komplette Stellglied-Pfad UI → Server → Lab-Actor →
Hardware, an dem sich spätere Stellglieder (DCG-Spannung etc.) orientieren.

**Hardware-Verifikation (2026-07-19, real gegen FW 3.71 auf Adresse 4,
`cargo run --example dds_probe -p octlab-transport`):**

- Subkanal 0 ist bestätigt die Frequenz in Hz: `4:0?` → `#4:0=1000.0`;
  `4:0=1234.5!` → Rücklesen liefert `#4:0=1234.5`. Die seit dem ersten
  Gerüst in `octlab-devices` stehende Unverifiziert-Warnung kann weg.
- Ein Set-Kommando mit `!` wird **unaufgefordert** mit einer Statuszeile auf
  Subkanal 255 quittiert: `#4:255=0 [OK]`. Explizites `4:255?` liefert
  denselben letzten Status.
- Fehlerstatus real beobachtet: `#4:255=5 [PARERR]` – bei Wert über dem
  DDS-Bereich (`4:0=99999999!`), bei negativem Wert (`4:0=-5!`) und bei
  unparsebarem Wert (`4:0=abc!`).
- **PARERR heißt NICHT "Wert unverändert":** Übergroßer Wert wurde auf
  `999999.8` geklemmt, negativer auf `0` – jeweils MIT PARERR-Quittung.
  Nur der unparsebare Wert ließ die Frequenz tatsächlich unverändert.
  Konsequenz: Der tatsächliche Zustand ist grundsätzlich nur per Rücklesen
  feststellbar, nie aus Wunschwert + Quittung ableitbar.
- **Obergrenze:** Maximal einstellbar ist `999999.8` Hz. `4:0=999999!` →
  OK; `4:0=1000000!` und `4:0=999999999!` → jeweils PARERR + Klemmung auf
  `999999.8`.
- **Rücklese-Auflösung:** Die Antwort formatiert mit EINER Nachkommastelle:
  `4:0=440.123!` → `#4:0=440.1`; `4:0=1234.5678!` → `#4:0=1234.5`;
  `4:0=1000.0005!` → `#4:0=1000.0`. Größte beobachtete Abweichung
  Wunsch↔Rücklesen: 0.0678 Hz (bei 1234.5678).

**Toleranz für den Vergleich Wunschwert ↔ Rücklesewert: absolut 0.1 Hz.**
Herleitung aus der Messung oben: Die Abweichung entsteht durch die
Rücklese-Formatierung auf eine Nachkommastelle (plus darunterliegende
Phasenakkumulator-Quantisierung, die bei einem DDS eine konstante
*absolute* Schrittweite hat – f = N·Δf) und ist damit über den gesamten
Frequenzbereich absolut beschränkt auf < 0.1 Hz, unabhängig von der
Frequenz. Eine *relative* Toleranz (z.B. 1e-6) wäre die falsche Form: bei
1234.5678 Hz ist die real gemessene Abweichung 5.5e-5 relativ – ein
relatives 1e-6 würde dieses völlig korrekte Setzen als Fehlschlag werten.
Umgekehrt braucht es oberhalb der 0.1 Hz keinen relativen Zuschlag, weil
die Formatierungs-Granularität nicht mit der Frequenz wächst (bei
999999 Hz kam exakt 999999.0 zurück). Liegt die Abweichung über 0.1 Hz,
hat die Anlage den Wert tatsächlich verändert (Klemmung o.ä.) – das ist
dem Bediener anzuzeigen (AK7).

**Entscheidung Kommando-Kanal UI → Server: HTTP-POST**, nicht bidirektionaler
WebSocket. Ein Setz-Vorgang ist ein Request/Response-Paar (Wunschwert rein,
Quittung + Rücklesewert raus) – HTTP liefert die Korrelation, die
Fehlersemantik (Statuscodes) und die curl-Testbarkeit geschenkt; ein
bidirektionaler WebSocket bräuchte dafür selbstgebaute Request-IDs und ein
eigenes Fehlerprotokoll. Der bestehende `/ws` bleibt reiner Messwert-Push –
die Richtungen bleiben sauber getrennt (POST: Client→Server-Kommandos,
WS: Server→Client-Broadcast). Auch ein späterer Sweep kippt die
Entscheidung nicht: der läuft laut Modell als Process-Manager im Server,
die UI startet/stoppt ihn nur – wieder Request/Response.

**Route, zu Ende gedacht:** `POST /api/channel/{addr}/{sub}` mit dem
Wunschwert im JSON-Body, Antwort enthält Quittungsstatus + Rücklesewert.
Generisch über Adresse/Subkanal, nicht DDS-spezifisch – die DCG-Spannung
ist später `POST /api/channel/2/<sub>` mit derselben Semantik, ohne neuen
Endpoint. Kein `GET` auf derselben Route in dieser Ausbaustufe (Lesen
läuft weiter über den WS-Push); Sweeps wären später eigene Ressourcen
(`/api/sweep/...`), keine Überladung dieser Route. Gebaut und im
Frontend verdrahtet wird in dieser Spec NUR der Frequenz-Fall
(Adresse 4, Subkanal 0).

Der Setz-Ablauf ist serverseitig zweistufig: Set-Kommando senden, Quittung
(Subkanal 255) abwarten, dann den Kanal rücklesen. Die Antwort an die UI
enthält Quittungsstatus UND zurückgelesenen Wert – wegen des
Klemm-Verhaltens (siehe oben) auch im Fehlerfall.

## Akzeptanzkriterien

### AK1: Erfolgreiches Setzen wird quittiert (Lab-Ebene)

Gegeben ein verbundenes Lab mit einem Modul, das Set-Kommandos mit `[OK]`
auf Subkanal 255 quittiert
Wenn das Setzen eines Kanalwerts angefordert wird
Dann geht das Set-Kommando im Draht-Format `<addr>:<sub>=<wert>!` raus und
das Ergebnis meldet Erfolg mit dem Statustext `OK`

### AK2: Ablehnende Quittung wird als solche gemeldet (Lab-Ebene)

Gegeben ein verbundenes Lab mit einem Modul, das Set-Kommandos mit
`#<addr>:255=5 [PARERR]` quittiert
Wenn das Setzen eines Kanalwerts angefordert wird
Dann meldet das Ergebnis den Statustext `PARERR` und ist von Erfolg
unterscheidbar

### AK3: Ausbleibende Quittung ist von Erfolg und Ablehnung unterscheidbar (Lab-Ebene)

Gegeben ein verbundenes Lab mit einem Modul, das auf Set-Kommandos gar
nicht antwortet
Wenn das Setzen eines Kanalwerts angefordert wird
Dann meldet das Ergebnis nach dem Query-Timeout "keine Antwort" – als
dritter Fall neben Erfolg und Ablehnung (kein Default-Wert, analog zur
`Option<f64>`-Entscheidung bei `query()`)

### AK4: Server-Endpoint setzt und liest zurück

Gegeben ein laufender Server (Simulation reicht)
Wenn per HTTP ein Setz-Request für einen Kanal (Adresse, Subkanal, Wert)
eintrifft und die Anlage mit `OK` quittiert
Dann enthält die HTTP-Antwort den Quittungsstatus und den anschließend
zurückgelesenen Kanalwert

### AK5: Server-Endpoint unterscheidet die Fehlerfälle

Gegeben ein laufender Server
Wenn die Anlage die Quittung verweigert (Fehlerstatus) oder gar nicht
antwortet
Dann ist die HTTP-Antwort in beiden Fällen kein Erfolgsfall (kein 2xx),
nennt den Grund (Statustext der Anlage bzw. Timeout) und enthält im
Quittungs-Fehlerfall trotzdem den zurückgelesenen Ist-Wert (wegen des
Klemm-Verhaltens der Firmware)

### AK6: UI zeigt die bestätigte Frequenz, nicht den Wunschwert

Gegeben das Frontend mit Eingabefeld und Setzen-Button neben dem Gauge
Wenn der Bediener eine Frequenz eingibt, setzt und die Anlage quittiert
Dann zeigt die UI die aus der Antwort stammende zurückgelesene Frequenz an
– weicht das Rücklesen höchstens um die Toleranz (absolut 0.1 Hz, siehe
Kontext) vom Wunschwert ab, gilt das Setzen als bestätigt und wird ohne
Warnung angezeigt (Beispiel: Wunsch 1234.5678, Rücklesen 1234.5)

### AK7: Abweichung über der Toleranz und Fehlerfälle werden angezeigt

Gegeben das Frontend mit einer zuletzt bestätigten Frequenz
Wenn ein Setz-Versuch abgelehnt wird (Statustext, z.B. PARERR), die
Anlage nicht antwortet, oder das Rücklesen um mehr als 0.1 Hz vom
Wunschwert abweicht (Klemmung – tritt real auch MIT Quittung auf)
Dann zeigt die UI den Grund an (Statustext, "keine Antwort" bzw. die
Abweichung) und die angezeigte bestätigte Frequenz springt nicht auf den
Wunschwert; liefert die Antwort einen Ist-Wert mit, wird dieser als
bestätigte Frequenz übernommen (z.B. 999999.8 nach Wunsch 2000000)

### AK8: Unsinnige Eingabe verlässt den Browser nicht

Gegeben das Frontend-Eingabefeld
Wenn der Bediener etwas Nicht-Numerisches eingibt und Setzen drückt
Dann wird kein Request gesendet und die UI kennzeichnet die Eingabe als
ungültig

### AK9: Live-Beweis am echten Gerät (manuell)

Gegeben der Server läuft mit `--connection tcp` gegen das reale c't-Lab
Wenn in der UI eine Frequenz (z.B. 2500 Hz) eingegeben und gesetzt wird
Dann quittiert das DDS mit OK und die UI zeigt die zurückgelesene Frequenz
(innerhalb 0.1 Hz um den Wunschwert); hängt ein Oszi/Frequenzzähler am
DDS-Ausgang, wird die Ausgangsfrequenz dort gegengeprüft, sonst gilt das
Rücklesen als Nachweis. Danach wird die Frequenz auf den vorherigen Wert
zurückgestellt (aktuell 1000.0 Hz)

## Explizit außerhalb des Scopes

- Drehknopf, Slider, Sweep – nur Eingabefeld + Button
- Weitere Stellglieder (DCG-Spannung, DDS-Pegel Subkanal 1) – der Endpoint
  ist generisch, aber UI und Live-Beweis gibt es nur für die DDS-Frequenz
- Client-seitige Bereichs-Validierung über "ist eine Zahl" hinaus – die
  Firmware klemmt selbst, das Rücklesen macht das sichtbar (verifiziert)
- Bidirektionaler WebSocket / Subscription-Umbau; der 500ms-DIV-Poll
  bleibt unangetastet
- Absicherung gegen konkurrierende Setz-Requests mehrerer Clients
  (Kiosk + Desktop gleichzeitig) – ein Bediener ist das Zielbild dieser
  Ausbaustufe

## Offene Fragen

- Keine – Subkanal-Zuordnung, Quittungs- und Klemm-Verhalten sind am
  realen Gerät verifiziert (siehe Kontext).
