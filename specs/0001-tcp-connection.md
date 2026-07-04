# 0001 – TCP-Verbindung zum XPort

Status: Entwurf

## Kontext

Bisher existiert nur `SimulatedConnection` als `BoardConnection`-Implementierung.
Für den Betrieb an echter Hardware braucht es eine Implementierung, die über
einen rohen TCP-Socket mit dem Lantronix-XPort-Baustein spricht (Standardport
10001, "raw"-Modus – kein Lantronix-eigener COM-Port-Treiber nötig, siehe
"c't-Lab: PC-Interface und Stromversorgung").

## Akzeptanzkriterien

### AK1: Erfolgreicher Verbindungsaufbau

Gegeben eine `TcpConnection` mit der Adresse eines TCP-Endpunkts, der Verbindungen annimmt
Wenn `connect()` aufgerufen wird
Dann kehrt der Aufruf ohne Fehler zurück

### AK2: Verbindung wird abgelehnt

Gegeben eine `TcpConnection` mit einer Adresse, an der niemand lauscht
Wenn `connect()` aufgerufen wird
Dann liefert der Aufruf `TransportError::Io` zurück, statt zu blockieren oder zu paniken

### AK3: Zeile senden

Gegeben eine erfolgreich verbundene `TcpConnection`
Wenn `send_line("0:IDN?")` aufgerufen wird
Dann empfängt die Gegenseite exakt die Bytes `0:IDN?\r\n`

### AK4: Vollständige Zeile empfangen

Gegeben eine erfolgreich verbundene `TcpConnection`
Wenn die Gegenseite `#0:254=1.28 [ADA by CM/c't 04/2007; Modules: IO]\r\n` sendet
Dann liefert `recv_line()` genau `#0:254=1.28 [ADA by CM/c't 04/2007; Modules: IO]`
zurück (ohne Zeilenende)

### AK5: Über mehrere TCP-Pakete fragmentierte Zeile

Gegeben eine erfolgreich verbundene `TcpConnection`
Wenn die Gegenseite dieselbe Antwort wie in AK4 in zwei separaten `write()`-Aufrufen
sendet (z.B. erst `#0:254=1.2`, dann `8 [ADA...]\r\n`)
Dann liefert `recv_line()` trotzdem eine einzige, korrekt zusammengesetzte Zeile
zurück – kein abgeschnittener oder doppelter Inhalt

## Explizit außerhalb des Scopes

- Automatisches Reconnect nach Verbindungsabbruch (eigene Spec, falls nötig)
- Discovery/Scan nach XPort-Geräten im Netzwerk
- TLS (das c't-Lab-Protokoll ist Klartext, XPort kann kein TLS)
- Lantronix-eigener "Virtual COM Port"-Treiber (bewusst umgangen, siehe Kontext)

## Offene Fragen

- Zeilenende: c't-Artikel nennen sowohl CR als auch CR/LF als Abschluss. AK3/AK4
  gehen von CR/LF aus (Analogie zur `ctlab.py`-Referenz, die `readline()` mit
  Standard-Zeilenende nutzt) – bei echtem Hardwaretest verifizieren und diese
  Spec ggf. korrigieren, bevor sie auf "Umgesetzt" gesetzt wird.
