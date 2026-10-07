# 0008 – Verbindungsverlust zum XPort

Status: In Arbeit

> Nummer 0007 ist für das Ziffernfeld der DDS-Ansicht reserviert (siehe
> Spec 0006, Entschiedene Fragen 1) und wird später vergeben.

## Kontext

Bricht die TCP-Verbindung zum XPort im laufenden Betrieb ab, wird der
Server heute unbrauchbar und belastet die Maschine:

**Beobachtung (2026-10-07, gegen `fake_xport`, nicht am echten XPort):**
Nach dem Beenden von `fake_xport` lief `octlab-server` in eine
Dauerschleife mit rund 100 % CPU auf einem Kern (gemessen: 304 CPU-Ticks
in 3 s bei 100 Ticks/s). Ursache: `TcpConnection::recv_line` liefert nach
dem Verbindungsende sofort und bei jedem Aufruf `Disconnected`, der
Lab-Actor ruft es ohne Pause erneut auf. Eine Wiederverbindung gibt es
nicht - der Server musste von Hand neu gestartet werden; ein Browser mit
offener Seite verlor dabei auch seine `/ws`-Verbindung.

Das ist dieselbe Fehlerklasse wie die Design-Entscheidung zu
`SimulatedConnection` in CLAUDE.md ("darf bei leerer Queue NIEMALS sofort
einen Err zurückgeben"), nur für die echte TCP-Verbindung und den Fall
"Gegenseite weg".

Diese Spec macht den Verbindungsverlust zu einem regulären Betriebszustand:
keine Dauerschleife, Kanäle sichtbar veraltet, Wiederverbindung mit
Backoff, Zustand abfragbar.

**Relevante Hardware-Fakten (CLAUDE.md, verifiziert):**

- **Der XPort akzeptiert nur eine aktive TCP-Session.** Ein neuer
  Connect-Versuch kurz nach dem Schließen der alten Session wurde live mit
  "Connection refused" abgewiesen - die alte Session ist serverseitig
  offenbar noch nicht abgebaut. Wiederverbinden muss die alte Session
  deshalb vollständig schließen und darf erst nach einer Wartezeit erneut
  verbinden. **Wie lang diese Wartezeit mindestens ist, ist unverifiziert**
  (Messpunkt 1).
- **Fail-fast beim Start** (`Lab::spawn` verbindet einmal vor dem Start
  des Actors, ein unerreichbarer XPort beendet den Server sofort) bleibt
  unverändert.

**Entschieden (Vorgabe Betreiber, 2026-10-07):**

- **Backoff für Wiederverbindungsversuche: Start 1 s, Faktor 2, Maximum
  30 s** (also 1, 2, 4, 8, 16, 30, 30, ... s). Das ist eine **Annahme,
  unverifiziert** - ob 1 s nach dem Schließen am XPort reicht, klärt
  Messpunkt 1.
- Fail-fast beim Start bleibt; Wiederverbindung gilt **nur für Abbrüche im
  laufenden Betrieb**.
- Der Verbindungszustand wird als **zusätzliches Feld in `GET /api/poll`**
  gemeldet (getrennt/verbunden, Zahl der Wiederverbindungsversuche seit dem
  Abbruch); das Frontend zeigt ihn **minimal** an.
- `octlab_transport::SimBus` bekommt eine **Simulation von Abbruch und
  Wiederverbindung**, damit sich die Kriterien unten mit pausierter Zeit
  deterministisch testen lassen (wie in Spec 0005).
- **HTTP-Status ohne Verbindung: 503** (Service Unavailable). 504 bleibt
  der Fall "Modul antwortet nicht / Quittung fehlt" (Spec 0003/0005).
- **Aufträge während der Unterbrechung werden sofort mit Fehler
  beantwortet** und NICHT bis zur Wiederverbindung zurückgehalten.
  Begründung: Ein verspätet ausgeführtes Setzen wäre bei einem Stellglied
  gefährlich - der Bediener hat dann längst etwas anderes im Sinn, und die
  Anlage würde unerwartet umschalten.
- **Felder in `GET /api/poll`:** `connection` (`"connected"` oder
  `"disconnected"`) und `reconnect_attempts` (Zahl der
  Wiederverbindungsversuche seit dem letzten Abbruch, 0 im verbundenen
  Zustand).

**Was als Abbruch gilt:** Die Verbindung meldet beim Lesen oder Senden
`Disconnected` bzw. einen I/O-Fehler (Gegenseite hat geschlossen oder
zurückgesetzt). Eine still hängende Verbindung ohne solche Meldung (z.B.
Kabel gezogen, kein FIN/RST) erkennt diese Spec NICHT - dort laufen wie
heute nur die Abfragen ins Timeout (siehe "Außerhalb des Scopes" und
Messpunkt 2).

## Akzeptanzkriterien

Gherkin wie in Spec 0005 (englische Schlüsselwörter, deutscher Text). Die
Zeiten sind Vorgaben bzw. Annahmen (Backoff), keine Hardware-Fakten; in
Tests mit pausierter Uhr exakt prüfbar.

### AK1: Keine Dauerschleife nach Verbindungsende

```gherkin
Scenario: Kein Dauerlesen nach Verbindungsende
  Given ein laufendes Lab mit Polling über eine Verbindung
  When die Gegenseite die Verbindung schließt
  Then liest der Actor nicht ununterbrochen weiter
  And bis zum nächsten Wiederverbindungsversuch wird auf der getrennten Verbindung weder gelesen noch gesendet
```

### AK2: Alle Kanäle werden veraltet, Werte bleiben

```gherkin
Scenario: Verbindungsverlust markiert die Kanalliste als veraltet
  Given die Kanäle 1:0 und 4:0 haben Werte
  When die Verbindung verloren geht
  Then sind 1:0 und 4:0 als veraltet markiert und zeigen ihre letzten Werte
  And der Änderungsstrom (und damit /ws) meldet beide Wechsel
```

### AK3: Setzen während der Unterbrechung scheitert schnell

```gherkin
Scenario: Setz-Sequenz ohne Verbindung
  Given die Verbindung ist verloren
  When das Setzen von 4:0 angefordert wird
  Then meldet das Ergebnis "keine Verbindung"
  And es wird nicht 500 + 500 ms auf Quittung und Rücklesen gewartet
  And nach einer späteren Wiederverbindung wird dieses Setzen NICHT nachträglich gesendet

Scenario: HTTP-Antwort ohne Verbindung
  Given die Verbindung ist verloren
  When POST /api/channel/4/0 eintrifft
  Then ist die Antwort 503, nennt die fehlende Verbindung als Grund
  And enthält keinen Wert
```

### AK4: Wiederverbindung mit Backoff

```gherkin
Scenario: Wachsende Abstände bis zum Maximum
  Given die Verbindung ist verloren und die Gegenseite nimmt keine Verbindung an
  When Zeit vergeht
  Then erfolgen Verbindungsversuche nach 1, 2, 4, 8, 16 und danach alle 30 s

Scenario: Alte Session wird vor dem nächsten Versuch geschlossen
  Given die Verbindung ist verloren
  When ein neuer Verbindungsversuch unternommen wird
  Then ist die vorige Session zu diesem Zeitpunkt vollständig geschlossen
```

### AK5: Wiederaufnahme nach erfolgreicher Verbindung

```gherkin
Scenario: Polling läuft nach der Wiederverbindung weiter
  Given die Verbindung war verloren
  When ein Verbindungsversuch gelingt
  Then läuft das Polling wieder an
  And die Kanäle verlieren die Markierung "veraltet" mit den ersten Antworten

Scenario: Backoff beginnt nach einem erneuten Abbruch wieder bei 1 s
  Given die Verbindung war verloren und wurde wiederhergestellt
  When die Verbindung erneut verloren geht
  Then erfolgt der erste neue Versuch wieder nach 1 s
```

### AK6: Verbindungszustand ist abfragbar

```gherkin
Scenario: Getrennt
  Given die Verbindung ist verloren und es gab 3 erfolglose Versuche
  When GET /api/poll abgefragt wird
  Then enthält die Antwort "connection": "disconnected" und "reconnect_attempts": 3
  And die bisherigen Felder (Intervall, Zykluszeit, Zyklen, Überläufe) sind unverändert vorhanden

Scenario: Verbunden
  Given die Verbindung steht
  When GET /api/poll abgefragt wird
  Then enthält die Antwort "connection": "connected" und "reconnect_attempts": 0
```

### AK7: Minimale Anzeige im Frontend

```gherkin
Scenario: Verbindungsverlust ist sichtbar
  Given das Frontend ist geöffnet und die Verbindung zum XPort geht verloren
  When der Zustand aktualisiert wird
  Then zeigt das Frontend sichtbar "keine Verbindung zur Anlage" an
  And nach der Wiederverbindung verschwindet die Anzeige
```

Ort und Form der Anzeige sind minimal (z.B. ein kurzer Text in der
Tab-Leiste, ohne ein Touch-Ziel zu verdrängen). Sie darf das Layout aus
Spec 0006 (800x480 ohne Scrollen, Touch-Ziele >= 44 px) nicht verletzen.

### AK8: Fail-fast beim Start bleibt

```gherkin
Scenario: XPort beim Start nicht erreichbar
  Given der XPort ist beim Start des Servers nicht erreichbar
  When octlab-server mit --connection tcp startet
  Then beendet er sich wie bisher sofort mit Fehlermeldung
  And es wird keine Wiederverbindung versucht
```

### AK9: Live-Beweis am echten XPort (manuell, offen bis zur Laborsession)

Gegeben der Server läuft mit `--connection tcp` gegen das reale c't-Lab
Wenn die Verbindung im Betrieb unterbrochen wird (z.B. XPort neu starten
bzw. kurz vom Netz nehmen)
Dann bleibt die CPU-Last des Servers niedrig, die Übersicht zeigt alle
Kanäle als veraltet, `GET /api/poll` meldet "getrennt" mit steigender
Versuchszahl, und nach Wiederkehr des XPort laufen die Werte ohne
Neustart des Servers wieder. Befunde siehe Messpunkte.

## Explizit außerhalb des Scopes

- **`/ws`-Wiederverbindung im Frontend** nach einem Server-Neustart -
  eigene, spätere **Spec 0009**. (Bei einem Abbruch zum XPort bleibt die
  `/ws`-Verbindung zum Server bestehen; dieser Fall braucht 0009 nicht.)
- **Überlauf-Zählung bei dauerhaft stummen Modulen** (beobachtet
  2026-10-07: zwei stumme Kanäle erzeugen alle 6 s eine neue
  Überlauf-Episode mit Warnung) - spätere Änderung an Spec 0005 AK10.
- Erkennen einer still hängenden Verbindung ohne FIN/RST (TCP-Keepalive,
  Watchdog über ausbleibende Antworten)
- Wiederverbindung beim Start (Fail-fast bleibt, AK8)
- Serielle Verbindung (`SerialConnection` existiert nicht)
- Backoff-Werte zur Laufzeit konfigurierbar machen

## Entschiedene Fragen (Betreiber, 2026-10-07)

1. **HTTP-Status ohne Verbindung:** 503; 504 bleibt für fehlende
   Quittung bzw. Antwort (AK3).
2. **Aufträge während der Unterbrechung:** sofort mit Fehler beantworten,
   nicht bis zur Wiederverbindung zurückhalten - ein verspätet
   ausgeführtes Setzen wäre bei einem Stellglied gefährlich (AK3).
3. **Felder in `GET /api/poll`:** `connection`
   (`"connected"`/`"disconnected"`) und `reconnect_attempts` (AK6).

## Offene Messpunkte für die nächste Laborsession

1. **Wartezeit des XPort nach dem Schließen bis zur neuen Session:** Eine
   Session schließen, dann im Abstand von z.B. 100 ms neu verbinden und die
   Zeit bis zum ersten Erfolg messen; mindestens 10 Wiederholungen,
   Minimum/Median/Maximum. Entscheidet, ob der Backoff-Start von 1 s
   ausreicht. (Kein laufender `octlab-server` währenddessen - nur eine
   Session gleichzeitig.)
2. **Was sieht der Server bei Verbindungsverlust am echten XPort?** XPort
   neu starten bzw. stromlos machen und Netzwerkkabel ziehen: kommt ein
   Verbindungsende (EOF/RST) beim Server an, oder hängt die Verbindung still
   (dann greift diese Spec nicht, siehe "Außerhalb des Scopes")?
   **Folgeaktion:** Tritt stilles Hängen auf, braucht es eine eigene Spec
   (TCP-Keepalive bzw. Erkennung über eine Zeitüberschreitung ohne
   Antworten).
3. **Verhalten nach XPort-Neustart:** Nimmt der XPort nach einem Neustart
   sofort wieder Verbindungen an, und antworten die Module danach ohne
   weiteres Zutun?
