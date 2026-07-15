# 0001 – TCP-Verbindung zum XPort

Status: Umgesetzt

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
Wenn die Gegenseite `#0:254=1.742 [ADA by CM/c't 04/2007; DA12 AD16 IO32 LCD ]\r\n` sendet
Dann liefert `recv_line()` genau `#0:254=1.742 [ADA by CM/c't 04/2007; DA12 AD16 IO32 LCD ]`
zurück (ohne Zeilenende, inklusive des Leerzeichens vor der schließenden `]`)

### AK5: Über mehrere TCP-Pakete fragmentierte Zeile

Gegeben eine erfolgreich verbundene `TcpConnection`
Wenn die Gegenseite dieselbe Antwort wie in AK4 in zwei separaten `write()`-Aufrufen
sendet (z.B. erst `#0:254=1.742 [ADA by CM/c`, dann `'t 04/2007; DA12 AD16 IO32 LCD ]\r\n`)
Dann liefert `recv_line()` trotzdem eine einzige, korrekt zusammengesetzte Zeile
zurück – kein abgeschnittener oder doppelter Inhalt

**Von echter Hardware bestätigt, nicht nur synthetisch getestet:** Beim
Hardwaretest von `*:IDN?` gegen den echten XPort ist dieser Fall organisch
aufgetreten, mitten im Wort: Der erste `read()` lieferte u.a.
`"...#2:254=2.92 [D"` (Modul-2-Zeile mitten in "DCG" abgeschnitten), der
zweite `read()` lieferte den Rest `"CG by CM/c't 05/2010]..."`. `recv_line()`
hat die Zeile trotzdem korrekt zu `#2:254=2.92 [DCG by CM/c't 05/2010]`
zusammengesetzt. Details siehe CLAUDE.md, Abschnitt "Verifizierte
Hardware-Fakten".

### AK6: Split mitten in einem Mehrbyte-UTF-8-Zeichen (synthetisch)

**Hypothetisch – auf der echten Hardware bisher nicht beobachtet** (alle
bisher beobachteten Klartexte sind reines ASCII, siehe AK4/AK5). Dieser Test
ist bewusst konstruiert, um den Byte-Puffer-Vertrag von `recv_line()`
explizit zu machen: der Puffer arbeitet auf Byte-Ebene, sucht das
Trennzeichen `0x0A`, und dekodiert erst zu `String`, wenn eine vollständige
Zeile vorliegt. Ergänzt AK5 (echte, beobachtete ASCII-Fragmentierung) um den
synthetischen Multibyte-Fall – zusammen decken beide Tests die Grenze
zwischen "auf dem Draht beobachtet" und "mit den Mitteln von Rusts
Typsystem/UTF-8-Garantien beweisbar korrekt" ab.

Gegeben eine erfolgreich verbundene `TcpConnection`
Wenn die Gegenseite die Zeile `#2:1=23.5 [Temperatur 23.5°C]\r\n` in zwei
separaten `write()`-Aufrufen sendet, wobei der Schnitt exakt zwischen den
beiden UTF-8-Bytes des Grad-Zeichens `°` liegt (`0xC2` im ersten,
`0xB0` im zweiten `write()`)
Dann liefert `recv_line()` trotzdem die korrekt zusammengesetzte, gültige
Zeile `#2:1=23.5 [Temperatur 23.5°C]` zurück – kein Panic, keine kaputte
UTF-8-Sequenz, kein Datenverlust

## Explizit außerhalb des Scopes

- Automatisches Reconnect nach Verbindungsabbruch (eigene Spec, falls nötig)
- Discovery/Scan nach XPort-Geräten im Netzwerk
- TLS (das c't-Lab-Protokoll ist Klartext, XPort kann kein TLS)
- Lantronix-eigener "Virtual COM Port"-Treiber (bewusst umgangen, siehe Kontext)

## Verifizierte Annahmen

- **Zeilenende: CR/LF (`\r\n`), bestätigt.** War als offene Frage markiert
  (c't-Artikel nennen sowohl CR als auch CR/LF), jetzt per Byte-Level-Test
  gegen den echten XPort geklärt: `xxd` auf die rohen TCP-Antwortbytes zeigt
  über mehrere Kommandos und alle vier Module hinweg konsistent `0d 0a`,
  keine Ausnahme. AK3/AK4/AK5s Annahme war korrekt, keine Korrektur nötig.
  Siehe auch CLAUDE.md, Abschnitt "Verifizierte Hardware-Fakten".

## Verifiziert gegen echte Hardware

AK1–AK6 sind grün gegen `SimulatedConnection`-basierte Loopback-TCP-Tests
(siehe `crates/octlab-transport/src/lib.rs`, Tests `ak1_*`–`ak6_*`). Zusätzlich
manuell gegen den echten XPort (`192.168.1.104:10001`) verifiziert via
`cargo run --example xport_probe -p octlab-transport`:

- `*:IDN?` liefert Echo + alle vier Module (Adressen 0/1/2/4), Werte passend
  zu CLAUDE.md "Verifizierte Hardware-Fakten".
- `1:VAL 0?` liefert einen plausiblen DIV-Messwert (`#1:0=0.0179526`,
  offene Klemmen, Wert schwankt zwischen Messungen – siehe AK-Beispiele oben).
- Die Fünf-Zeilen-`*:IDN?`-Antwort kam über drei separate `read()`-Aufrufe
  herein (97 + 36 + 46 Bytes), inklusive eines mitten im Wort geschnittenen
  Falls (AK5, siehe dort) – `recv_line()` hat alle fünf Zeilen korrekt
  rekonstruiert.
