# 0005 – Mehrkanal-Ansicht (rein lesend)

Status: In Arbeit

## Kontext

Bisher sieht der Bediener genau einen Messwert (DIV, Adresse 1, Subkanal 0),
getaktet vom provisorischen 500ms-Poll `poll_div_provisional` (siehe
CLAUDE.md, "Nächste Schritte" Schritt 3, und Spec 0002), plus die
DDS-Frequenz im Bedienfeld aus Spec 0003. Die Anlage hat aber vier Module
(ADA-IO 0, DIV 1, DCG 2, DDS 4; Adresse 3 ist unbelegt), und für einen
Messplatz muss man mehrere Größen gleichzeitig im Blick haben. Diese Spec
bringt eine **rein lesende** Übersicht über eine feste Liste von Kanälen
aller vier Module. Sie ersetzt den provisorischen Einzelkanal-Poll durch
einen Poll-Zyklus über diese Liste, der den Ausfall einzelner Kanäle
verkraftet.

**Entschiedene Randbedingungen (Vorgabe Betreiber, 2026-10-06):**

- Module: ADA-IO (0), DIV (1), DCG (2), DDS (4), nur lesend. Adresse 3
  entfällt.
- Die Kanalliste ist **statisch** und liegt in der **Server-Konfiguration**.
  Es gibt keine dynamische Erkennung, welche Subkanäle ein Modul hat, und
  kein ESDM-Read-Model.
- Polling: Ziel ist ein voller Zyklus über alle Kanäle mit 1 Hz. Das
  Intervall ist konfigurierbar, Default 1000 ms, **0 = Polling aus**. Das
  Polling läuft auf **jeder** Verbindungsart (TCP und Simulation).
- Dauert ein Zyklus länger als das Intervall, gilt: Zyklen überlappen
  nicht, verpasste Takte werden nicht nachgeholt, der nächste Zyklus
  startet sofort. Pro Überlauf-Episode gibt es genau eine Warnung. Die
  gemessene Zykluszeit ist über den Server abrufbar.
- Ein Kanal, dessen Abfrage ins Timeout (500 ms, bestehendes
  Query-Timeout) läuft, wird als **veraltet** markiert. Der nächste Versuch
  für diesen Kanal kommt frühestens nach **5 s Backoff**.
- Setzen hat Vorrang vor Poll-Abfragen. Die Setz-Sequenz aus Spec 0003
  (Setzen, Quittung, Rücklesen) ist im Lab-Actor **atomar**: Zwischen
  ihren Schritten geht keine Poll-Abfrage auf die Leitung.
- Push über `/ws` **nur bei Änderung**, adressiert über Adresse +
  Subkanal, rückwärtskompatibel zum Nachrichtenformat aus Spec 0002. Der
  Änderungsfilter gilt nur für Kanäle der Kanalliste. Neue Clients
  erhalten beim Verbinden den aktuellen Stand (Snapshot).
- Quittungen (Subkanal 255) werden ausschließlich vom Setz-Vorgang
  ausgewertet. Unaufgeforderte Zeilen werden für `/ws` geloggt und
  verworfen.
- Die Anlage ist bei der Erstellung dieser Spec nicht verfügbar. Alles zu
  Kanälen, Einheiten, Bereichen und Zeiten gilt deshalb als
  **unverifiziert (Hardware-Messung offen)**, außer es steht bereits als
  verifiziert in CLAUDE.md bzw. Spec 0003.

**Relevante Hardware-Fakten (CLAUDE.md, verifiziert):**

- **Nur eine XPort-Session:** Das Polling läuft über die eine bestehende
  Verbindung des Lab-Actors. Es baut keine zweite Verbindung auf. Die
  Diagnose-Tools (`xport_probe`, `dds_probe`) können deshalb nicht parallel
  zum laufenden Server messen (siehe Messpunkte unten).
- **Echo:** Adressierte Abfragen (`1:0?`) erzeugen kein Echo. Das Polling
  verwendet ausschließlich adressierte Abfragen, kein `*:`-Broadcast.
  Trotzdem auftauchende Fremdzeilen ohne `#` werden wie bisher verworfen
  (`malformed_input.feature`).
- **Stellglied-Regel:** Hier nur indirekt relevant (diese Spec liest nur).
  Ein Setz-Vorgang aus Spec 0003 (Kommando → Quittung auf 255 → Rücklesen)
  läuft weiterhin vollständig ab und darf vom Polling nicht gestört
  werden, siehe AK6.
- **Unaufgeforderte Zeilen** (Panel-Bedienung, Statusmeldungen auf
  Subkanal 255) kommen jederzeit, auch während des Pollings. Ab dieser
  Spec werden sie **nicht mehr über `/ws` gepusht**, sondern geloggt und
  verworfen. Ändert jemand am Panel einen Wert auf einem Kanal der
  Kanalliste, zeigt die Übersicht ihn erst nach der nächsten Poll-Antwort.
  Die Lab-interne Verteilung an Abonnenten
  (`unsolicited_updates.feature`) bleibt unverändert. Sie ist eine
  Eigenschaft der Lab-API, nicht des `/ws`-Vertrags.

### Kanalmodell (Stand 2026-10-06, Recherche ohne Hardware)

**Quellen:**

- **[S2010]** "c't-Lab Syntax Stand 21.05.2010", `syntax.pdf`/`syntax.xls`
  von https://www.sn7400.de/ctlab/Dokumentation/ (lokale Referenz, NICHT
  im Repo, Lizenz unklar). Abschnitte: ADA-IO 1.74, DDS 3.70, DCG 2.9,
  DIV 3.04.
- **[LAT]** `status_latenzen.pdf`, gleiche Quelle (ADA-IO-Latenzen,
  Statusbits).
- **[HW]** Verifizierte Hardware-Fakten in CLAUDE.md bzw. Spec 0003.
- Forum (https://ctlabforum.thoralt.de): Die Suche fand keine Syntax-Doku
  für neuere Firmware-Stände.

**Firmware-Stände: Gerät vs. Doku**

| Adr | Modul  | FW am Gerät [HW] | FW in [S2010] | Abstand |
|-----|--------|------------------|---------------|---------|
| 0   | ADA-IO | 1.742            | 1.74          | Patch-Level |
| 1   | DIV    | 3.10             | 3.04          | **sechs Minor-Stände, Doku für 3.10 fehlt** |
| 2   | DCG    | 2.92             | 2.9           | Patch-Level |
| 4   | DDS    | 3.71             | 3.70          | Patch-Level, Subkanal 0 am Gerät verifiziert |

Alle Kanäle unten sind in dieser Spec **nur lesend**. Die Spalte "Art lt.
Doku" sagt, was der Kanal laut [S2010] ist: ein reiner Messwert (`**` =
"nur Lesen") oder ein Soll-/Einstellwert, der sich auch lesen lässt.
"unverifiziert" heißt hier immer: **unverifiziert (Hardware-Messung
offen)**.

#### ADA-IO (Adresse 0; IDN: `DA12 AD16 IO32 LCD`)

| Subk.  | Name           | Einheit | Wertebereich | Art lt. Doku | Quelle | Status |
|--------|----------------|---------|--------------|--------------|--------|--------|
| 0..7   | VAL, AD10 (ATmega-intern, 10 Bit) | V | 0..10 (unipolar) | Messwert | [S2010], [LAT] | unverifiziert |
| 10..17 | VAL, AD16-8 (16 Bit, Steckkarte) | V | −10..+10 | Messwert | [S2010], [LAT] | unverifiziert |
| 20..27 | VAL, DA12-8 (Ausgabe) | V | −10..+10 | Sollwert. Lesen liefert vermutlich den Sollwert, keine Messung | [S2010] | unverifiziert |
| 30..37 | PIO, IO-Port-Datenbyte | – (Byte) | 0..255 | Port-Datenbyte, Richtung über DIR 40..47 | [S2010], [LAT] | unverifiziert |

#### DIV (Adresse 1)

| Subk. | Name | Einheit | Wertebereich | Art lt. Doku | Quelle | Status |
|-------|------|---------|--------------|--------------|--------|--------|
| 0  | VAL, ADC24 skaliert | **V oder A, DC oder AC-TrueRMS, abhängig von RNG (19)** | je nach RNG, 250 mV..250 V bzw. 25 mA..10 A; Overload = `-99999` | Messwert | [S2010], [HW] | **teilverifiziert:** antwortet, z.B. `#1:0=0.0022024` [HW]. Einheit, Bereich und Overload-Kennung unverifiziert |
| 1  | VAL, integrierter Messwert | wie 0 | wie 0 | Messwert | [S2010] | unverifiziert |
| 2  | VAL, langsam integrierter Messwert | wie 0 | wie 0 | Messwert | [S2010] | unverifiziert |
| 10 | VAL, ADC10, AC-TrueRMS oder DC | wie 0 | wie 0 | Messwert | [S2010] | unverifiziert |
| 11 | VAL, ADC10, AC-Peak oder DC | wie 0 | wie 0 | Messwert | [S2010] | unverifiziert |
| 19 | RNG, Messbereich | – (Code) | 0..15: DC-U 0..3, AC-U 4..7, DC-I 8..11, AC-I 12..15 | Einstellwert, lesbar | [S2010] | unverifiziert |

#### DCG (Adresse 2)

| Subk. | Name | Einheit | Wertebereich | Art lt. Doku | Quelle | Status |
|-------|------|---------|--------------|--------------|--------|--------|
| 0   | DCV, U Soll | V | offen (OPT 6 "Umax" Default 20) | Sollwert, lesbar | [S2010] | unverifiziert |
| 1   | DCA, I Soll | A | offen | Sollwert, lesbar | [S2010] | unverifiziert |
| 7   | MAH, kumulierter Strom | Ah | ≥ 0, kumulativ | Messwert | [S2010] | unverifiziert |
| 8   | MWH, kumulierte Leistung | Wh | ≥ 0, kumulativ | Messwert | [S2010] | unverifiziert |
| 10  | MSV, U Ist | V | offen | Messwert `**` | [S2010] | unverifiziert |
| 11  | MSA, I Ist | A | offen | Messwert `**` | [S2010] | unverifiziert |
| 12  | MSA 1, I Ist | mA | offen | Messwert `**` | [S2010] | unverifiziert |
| 13  | MSA 2, I Ist | µA | offen | Messwert `**` | [S2010] | unverifiziert |
| 15  | MSA 4, ungeregelte Eingangsspannung | V | offen | Messwert `**` | [S2010] | unverifiziert |
| 18  | MSW, abgegebene Leistung | W | offen | Messwert `**` | [S2010] | unverifiziert |
| 233 | TMP, Kühlkörpertemperatur | °C | offen | Messwert `**` | [S2010] | unverifiziert |

#### DDS (Adresse 4)

| Subk. | Name | Einheit | Wertebereich | Art lt. Doku | Quelle | Status |
|-------|------|---------|--------------|--------------|--------|--------|
| 0  | FRQ, Frequenz | Hz | 0..999999.8, Rücklesen mit 1 Nachkommastelle | Sollwert, lesbar | [HW] (Spec 0003), [S2010] | **verifiziert** (2026-07-19) |
| 1  | LVL, Pegel | mVeff | offen (Skalierungsbereiche 0..199 / 200..8000 mVeff) | Sollwert, lesbar | [S2010], `octlab-devices` | unverifiziert |
| 4  | WAV, Kurvenform | – (Code) | 0..5 (0 aus, 1 Sinus, 2 Dreieck, 3 Rechteck, 4 Logik, 5 extern) | Sollwert, lesbar | [S2010] | unverifiziert |
| 20 | DCO, DC-Offset | V | −10..+10 (5-mV-Schritte) | Sollwert, lesbar | [S2010] | unverifiziert |
| 10..12 | INL, Eingangspegel (TRMSC-Tochterplatine) | Veff / Vss / dB | offen | Messwert `**` | [S2010] | unverifiziert. **Ob die Tochterplatine bestückt ist, ist unbekannt** |

**Lücken (bewusst nicht aufgefüllt):**

- **DIV 3.10 ist nicht dokumentiert.** Die gesamte DIV-Belegung stammt aus
  der Doku für 3.04. Sie gilt erst nach Messung als belegt.
- **Einheit von DIV 0 ist nicht statisch.** Sie hängt vom Messbereich
  (RNG, Subkanal 19) ab, und der Bereich lässt sich laut Doku am Panel
  umschalten. Eine statische Einheit in der Kanalliste wäre für diesen
  Kanal falsch, sobald jemand am Gerät umschaltet.
- **Anzahl der IO-Ports.** "IO32" deutet auf 4 Ports hin (Subkanäle
  30..33), die Doku adressiert aber 0..7. Welche Ports antworten, ist
  offen.
- **Lesen von Sollwert-Kanälen** (ADA DA 20..27, DCG 0/1, DDS 1/4/20): Ob
  und in welchem Format diese auf `?` antworten, ist nur für DDS 0
  verifiziert.
- **Antwortzeiten.** Nur für ADA-IO gibt es Doku-Werte ([LAT]: ≤ 1 ms bis
  zur Antwort, AD-Messzeit max. 16 ms, IO 400..800 µs). Für DIV, DCG und
  DDS gibt es keine. Ob 1 Hz für die gewählte Kanalliste reicht, ist
  deshalb offen.
- **Nachkommastellen je Kanal.** Nur DDS 0 ist bekannt (eine Stelle).
  Das ist relevant für "Änderung" (AK7): Bei verrauschten Kanälen wird
  praktisch jeder Zyklus gepusht.
- **DCG-Wertebereiche.** Die Doku nennt nur Default-Grenzwerte aus dem
  EEPROM (OPT-Parameter), nicht den tatsächlichen Aufbau.
- **AD10 mit Panel.** Es gibt einen Forenthread mit dem Titel "AD10 bringt
  komische Werte, wenn das Panel angeschlossen ist" (nur der Titel ist
  gesichtet). Das LCD ist bestückt. Ob das Problem hier auftritt, ist
  offen.

**Widersprüche:**

- **Echo:** [S2010] sagt "c't-Lab liefert kein Echo". [HW] zeigt ein Echo
  bei Broadcast-Kommandos (`*:IDN?`). Es gilt [HW]. Für diese Spec ist das
  folgenlos, weil das Polling keine Broadcasts sendet.
- **IDN-Subkanal:** In [S2010] steht als Beispielantwort `#0:255=1.39
  [ADA-IO by c't]` (Subkanal 255). [HW] beobachtet `#0:254=1.742 [...]`
  (Subkanal 254). Es gilt [HW].
- **DCG-Beispiele in [S2010] passen nicht zur eigenen Tabelle:** MSV wird
  als `MSV?, 2?` gezeigt (Subkanal 2 ist laut Tabelle DCA in mA), MSA als
  `MSA?, 4:3?` (Subkanal 3 ist laut Tabelle DCA in µA). Die
  Tabellenspalte (10/11) ist plausibler, aber unverifiziert. Außerdem
  heißt der Abschnitt "hier für Adresse 4", die Beispielantworten nutzen
  aber `#1:`.
- **DDS Subkanal 5:** [S2010] belegt ihn doppelt, mit BST (Burst) und mit
  PWR ("nur für CMs Laborgerät"). Er ist nicht in der Kanalliste.
- **DIV OFS:** Laut [S2010] liegt OFS bei "Subkanal 100..104" für 16
  Bereiche, das sind nur 5 Nummern für 16 Werte. Für diese Spec ist das
  irrelevant (Kalibrierparameter), es zeigt aber die Qualität der Quelle.

### Kanalliste für diese Spec (festgelegt 2026-10-06)

Ein bewusst kleiner Startumfang: wenige Kanäle je Modul, die für einen
Messplatz die naheliegenden sind. Die Liste ist festgelegt, ihre
Belegung (Subkanäle, Einheiten) bleibt aber unverifiziert, bis die
Messpunkte erledigt sind. Die Einheit von DIV 0 wird statisch als
"abhängig von RNG" geführt.

| Adr | Subk. | Anzeige-Name | Einheit (lt. Doku, unverifiziert) |
|-----|-------|--------------|-----------------------------------|
| 0 | 10..13 | ADA AD16 Kanal 0..3 | V |
| 1 | 0  | DIV Messwert | abhängig von RNG |
| 1 | 19 | DIV Messbereich | Code |
| 2 | 10 | DCG U Ist | V |
| 2 | 11 | DCG I Ist | A |
| 2 | 0  | DCG U Soll | V |
| 2 | 1  | DCG I Soll | A |
| 4 | 0  | DDS Frequenz | Hz (verifiziert) |
| 4 | 1  | DDS Pegel | mVeff |

Das sind 13 Kanäle. Die Akzeptanzkriterien unten sind unabhängig von der
konkreten Liste formuliert.

## Akzeptanzkriterien

Die Szenarien sind als Gherkin notiert (englische Schlüsselwörter,
deutscher Text, wie in `crates/octlab-lab/tests/features/`). Die Zeiten
(1000 ms Intervall, 500 ms Timeout, 5 s Backoff) sind Vorgaben, keine
Hardware-Fakten. Sie lassen sich in Tests mit pausierter Zeit
deterministisch prüfen.

### AK1: Mehrere Kanäle werden zyklisch abgefragt und angezeigt

```gherkin
Scenario: Alle konfigurierten Kanäle liefern Werte
  Given eine Kanalliste mit den Kanälen 1:0, 2:10 und 4:0
  And ein Poll-Intervall von 1000 ms
  And alle drei Module antworten auf Abfragen
  When ein Poll-Zyklus abgelaufen ist
  Then wurde jeder der drei Kanäle genau einmal mit "<addr>:<sub>?" abgefragt
  And die Übersicht zeigt für jeden Kanal den zuletzt empfangenen Wert
  And kein Kanal ist als veraltet markiert
```

### AK2: Poll-Intervall ist konfigurierbar

```gherkin
Scenario: Abweichendes Intervall
  Given eine Kanalliste mit dem Kanal 1:0
  And ein Poll-Intervall von 2000 ms
  When 6000 ms vergehen
  Then wurde der Kanal 1:0 dreimal abgefragt

Scenario: Default-Intervall
  Given eine Kanalliste mit dem Kanal 1:0
  And kein Poll-Intervall ist konfiguriert
  When 3000 ms vergehen
  Then wurde der Kanal 1:0 dreimal abgefragt

Scenario: Intervall 0 schaltet das Polling ab
  Given eine Kanalliste mit dem Kanal 1:0
  And ein Poll-Intervall von 0 ms
  When 5000 ms vergehen
  Then wurde keine Poll-Abfrage gesendet
  And kein Kanal ist als veraltet markiert
```

### AK3: Ein Kanal timeoutet, die anderen bleiben aktuell

```gherkin
Scenario: Ausfall eines einzelnen Kanals
  Given eine Kanalliste mit den Kanälen 1:0, 2:10 und 4:0
  And die Module an Adresse 1 und 4 antworten auf Abfragen
  And das Modul an Adresse 2 antwortet nicht
  When drei Poll-Zyklen abgelaufen sind
  Then ist der Kanal 2:10 als veraltet markiert
  And die Kanäle 1:0 und 4:0 wurden in jedem der drei Zyklen abgefragt
  And die Kanäle 1:0 und 4:0 sind nicht als veraltet markiert
```

### AK4: Backoff nach Timeout, danach Wiederaufnahme

```gherkin
Scenario: Kein erneuter Versuch vor Ablauf des Backoffs
  Given eine Kanalliste mit dem Kanal 2:10
  And das Modul an Adresse 2 antwortet nicht
  When die Abfrage von 2:10 ins Timeout läuft
  Then wird 2:10 in den folgenden 5 s nicht erneut abgefragt

Scenario: Wiederaufnahme nach dem Backoff
  Given der Kanal 2:10 ist nach einem Timeout als veraltet markiert
  And das Modul an Adresse 2 antwortet wieder mit "#2:10=12.0"
  When der Backoff von 5 s abgelaufen ist und der nächste Poll-Zyklus läuft
  Then wird 2:10 wieder abgefragt
  And die Übersicht zeigt für 2:10 den Wert 12.0
  And 2:10 ist nicht mehr als veraltet markiert
  And 2:10 wird danach wieder in jedem Zyklus abgefragt

Scenario: Erneutes Timeout nach dem Backoff
  Given der Kanal 2:10 ist nach einem Timeout als veraltet markiert
  And das Modul an Adresse 2 antwortet weiterhin nicht
  When der Backoff abgelaufen ist und der Wiederholversuch ins Timeout läuft
  Then bleibt 2:10 als veraltet markiert
  And der nächste Versuch kommt wieder frühestens 5 s später
```

### AK5: Veralteter Wert bleibt sichtbar und ist markiert

```gherkin
Scenario: Letzter Wert bleibt stehen, mit Markierung
  Given der Kanal 2:10 hat zuletzt den Wert 12.0 geliefert
  When die nächste Abfrage von 2:10 ins Timeout läuft
  Then zeigt die Übersicht für 2:10 weiterhin 12.0
  And der Wert ist sichtbar als veraltet gekennzeichnet
  And der Wert wird weder durch 0 noch durch eine leere Anzeige ersetzt

Scenario: Kanal hat noch nie geantwortet
  Given der Kanal 2:10 hat seit Serverstart noch keinen Wert geliefert
  When die Abfrage von 2:10 ins Timeout läuft
  Then zeigt die Übersicht für 2:10 "kein Wert" und nicht die Zahl 0
  And der Kanal ist als veraltet gekennzeichnet
```

Begründung zum zweiten Szenario: Gleiche Linie wie `query() ->
Option<f64>` in CLAUDE.md. "Kein Wert" darf nicht mit dem gültigen
Messwert 0 verwechselbar sein.

### AK6: Setz-Sequenz ist atomar und hat Vorrang vor dem Polling

Die Setz-Sequenz aus Spec 0003 besteht aus drei Schritten: Setzen
(`<addr>:<sub>=<wert>!`), Quittung (Subkanal 255) und Rücklesen
(`<addr>:<sub>?`). Der Lab-Actor führt sie als **eine** unteilbare
Einheit aus.

```gherkin
Scenario: Setz-Sequenz überholt anstehende Poll-Abfragen
  Given eine Kanalliste mit mehreren Kanälen und laufendem Polling
  When während eines Poll-Zyklus das Setzen von 4:0 auf 2500 angefordert wird
  Then geht "4:0=2500!" vor jeder noch nicht gesendeten Poll-Abfrage auf die Leitung
  And höchstens eine bereits gesendete Poll-Abfrage geht ihm voraus

Scenario: Keine Poll-Abfrage innerhalb der Setz-Sequenz
  Given der Kanal 4:0 steht in der Kanalliste und das Polling läuft
  And das Modul an Adresse 4 quittiert mit "#4:255=0 [OK]" und liefert beim Rücklesen "#4:0=2500.0"
  When die Setz-Sequenz für 4:0 mit dem Wert 2500 läuft
  Then stehen "4:0=2500!" und "4:0?" (Rücklesen) direkt hintereinander im Sende-Log
  And zwischen Setz-Kommando, Quittung und Rücklese-Antwort wurde keine Poll-Abfrage gesendet
  And das Ergebnis (Quittung "OK", Rücklesewert 2500.0) entspricht Spec 0003
  And danach läuft das Polling ohne manuellen Eingriff weiter

Scenario: Atomarität gilt auch bei ausbleibender Quittung
  Given das Polling läuft
  And das Modul an Adresse 4 quittiert Setz-Kommandos nicht
  When die Setz-Sequenz für 4:0 läuft
  Then wird bis zum Ende der Setz-Sequenz keine Poll-Abfrage gesendet
  And die Setz-Sequenz blockiert das Polling höchstens 500 ms (Quittung) + 500 ms (Rücklesen)
  And das Ergebnis meldet "keine Antwort" wie in Spec 0003
  And danach läuft das Polling weiter
```

Ohne Quittung blockiert die Setz-Sequenz das Polling also bis zu 500 ms
(Quittungs-Timeout) plus 500 ms (Rücklese-Timeout). Dass sich der
laufende Poll-Zyklus dadurch verlängert, ist **erwartet** und kein
Fehler. Dauert er dadurch länger als das Intervall, greift AK10.

```gherkin
Scenario: Rücklesewert erscheint auch in der Übersicht
  Given der Kanal 4:0 steht in der Kanalliste mit dem Wert 1000.0
  When ein Setz-Vorgang für 4:0 mit Rücklesewert 2500.0 abgeschlossen ist
  Then zeigt die Übersicht für 4:0 den Wert 2500.0
```

Begründung: Quittung (Adresse/255) und Rücklesen (Abfrage auf genau dem
gesetzten Kanal) warten beide auf bestimmte Adresse-/Subkanal-Antworten.
Eine dazwischengeschobene Poll-Abfrage auf denselben Kanal würde um
dieselbe Antwort konkurrieren.

### AK7: Push über `/ws` nur bei Änderung

```gherkin
Scenario: Erster Wert wird gepusht
  Given ein verbundener WebSocket-Client
  When der Kanal 1:0 erstmals den Wert 0.0022 liefert
  Then erhält der Client eine Nachricht für Adresse 1, Subkanal 0 mit Wert 0.0022

Scenario: Unveränderter Wert wird nicht erneut gepusht
  Given der Kanal 4:0 hat zuletzt 1000.0 geliefert und das wurde gepusht
  When der nächste Poll-Zyklus für 4:0 wieder 1000.0 liefert
  Then erhält der Client für 4:0 keine neue Nachricht

Scenario: Geänderter Wert wird gepusht
  Given der Kanal 4:0 hat zuletzt 1000.0 geliefert
  When der nächste Poll-Zyklus für 4:0 den Wert 1000.5 liefert
  Then erhält der Client eine Nachricht für 4:0 mit Wert 1000.5

Scenario: Wechsel nach "veraltet" und zurück wird gepusht
  Given der Kanal 2:10 hat zuletzt 12.0 geliefert
  When die Abfrage von 2:10 ins Timeout läuft
  Then erhält der Client eine Nachricht für 2:10, die ihn als veraltet kennzeichnet
  When 2:10 nach dem Backoff wieder 12.0 liefert
  Then erhält der Client eine Nachricht für 2:10, die die Kennzeichnung aufhebt

Scenario: Neuer Client bekommt den aktuellen Stand
  Given die Kanäle 1:0 und 4:0 haben Werte, 2:10 ist veraltet
  When sich ein neuer WebSocket-Client verbindet
  Then erhält er sofort für jeden Kanal mit bekanntem Zustand eine Nachricht
```

Begründung zum letzten Szenario: Ohne diesen Stand würde ein Client, der
nach dem letzten Wechsel verbindet, einen konstanten Kanal (z.B. die
DDS-Frequenz) nie zu sehen bekommen. Bei "Push nur bei Änderung" ist das
die zwingende Ergänzung.

### AK8: Rückwärtskompatibel zu Spec 0002

```gherkin
Scenario: Client nach Spec 0002 versteht Wertnachrichten weiterhin
  Given ein Frontend-Zustand nach Spec 0002 (letzter Wert pro Kanal)
  When eine Wertnachricht aus dieser Spec für 1:0 mit Wert 1.5 eintrifft
  Then enthält der Zustand für 1:0 den Wert 1.5

Scenario: Veraltet-Nachricht verfälscht keinen Wert bei einem Client nach Spec 0002
  Given ein Frontend-Zustand nach Spec 0002 mit dem Wert 12.0 für 2:10
  When eine Nachricht eintrifft, die 2:10 als veraltet kennzeichnet
  Then enthält der Zustand für 2:10 keinen anderen Zahlenwert als 12.0
```

Die bestehenden Instrumente (DIV-Gauge aus Spec 0002, DDS-Bedienfeld aus
Spec 0003) funktionieren unverändert weiter. Der DIV-Kanal 1:0 wird
dafür künftig vom Poll-Zyklus versorgt statt vom 500ms-Provisorium.
**Sichtbare Folge:** Das Gauge aktualisiert sich mit 1 Hz statt 2 Hz.

### AK9: Unaufgeforderte und unbekannte Zeilen werden verworfen

```gherkin
Scenario: Unaufgeforderte Statuszeile während des Pollings
  Given eine Kanalliste mit dem Kanal 1:0 und laufendem Polling
  And ein verbundener WebSocket-Client
  When das Modul an Adresse 2 unaufgefordert eine Statuszeile auf Subkanal 255 sendet
  Then erhält der Client dafür keine Nachricht
  And die Zeile ist im Log vermerkt
  And das Polling für 1:0 läuft unverändert weiter

Scenario: Unaufgeforderter Wert für einen Kanal der Kanalliste
  Given der Kanal 4:0 steht in der Kanalliste mit dem Wert 1000.0
  And ein verbundener WebSocket-Client
  When das Modul an Adresse 4 unaufgefordert "#4:0=440.0" sendet
  Then erhält der Client dafür keine Nachricht
  And die Übersicht zeigt für 4:0 weiter 1000.0, bis eine Poll-Antwort einen neuen Wert liefert

Scenario: Quittungen erscheinen nicht auf /ws
  Given ein verbundener WebSocket-Client
  When eine Setz-Sequenz mit Quittung "#4:255=0 [OK]" abläuft
  Then erhält der Client keine Nachricht für Subkanal 255

Scenario: Echo- oder Müllzeile während des Pollings
  Given eine Kanalliste mit dem Kanal 1:0 und laufendem Polling
  When eine Zeile ohne "#"-Präfix empfangen wird
  Then wird sie verworfen und 1:0 wird weiter normal abgefragt

Scenario: Lab-Ebene verteilt unaufgeforderte Zeilen weiter, ohne den Kanalzustand zu ändern
  Given der Kanal 4:0 steht in der Kanalliste mit dem Wert 1000.0
  And ein Abonnent der Lab-Updates (subscribe)
  When das Modul an Adresse 4 unaufgefordert "#4:0=440.0" sendet
  Then empfängt der Abonnent den Wert 440.0
  And der Kanalzustand von 4:0 ist weiterhin 1000.0
  And /ws pusht dafür keine Nachricht
```

Abgrenzung: Auf Lab-Ebene verteilt `subscribe()` unaufgeforderte Zeilen
weiterhin an alle Abonnenten. `unsolicited_updates.feature` bleibt
unverändert und grün. Diese Zeilen ändern aber den Kanalzustand der
Übersicht nicht, und `/ws` pusht sie nicht. "Verworfen" bezieht sich
also auf den Kanalzustand und den `/ws`-Vertrag, nicht auf die Lab-API.

### AK10: Zyklus länger als das Intervall

```gherkin
Scenario: Kein Überlappen, kein Nachholen
  Given eine Kanalliste mit drei Kanälen
  And ein Poll-Intervall von 1000 ms
  And jedes Modul antwortet erst nach 400 ms
  When 6000 ms vergehen
  Then beginnt kein Zyklus, bevor der vorherige beendet ist
  And jeder Zyklus beginnt unmittelbar nach dem Ende des vorherigen
  And es laufen höchstens 5 Zyklen, verpasste Takte werden nicht nachgeholt

Scenario: Eine Warnung pro Überlauf-Episode
  Given ein Poll-Intervall von 1000 ms
  And mehrere Zyklen hintereinander dauern länger als 1000 ms
  Then wird genau eine Warnung geloggt
  When danach ein Zyklus wieder innerhalb des Intervalls bleibt
  And später erneut ein Zyklus länger als 1000 ms dauert
  Then wird für die neue Episode wieder genau eine Warnung geloggt

Scenario: Gemessene Zykluszeit ist abrufbar
  Given das Polling läuft und der letzte Zyklus dauerte 1200 ms
  When die Zykluszeit über den Server abgefragt wird
  Then enthält die Antwort die gemessene Dauer des letzten Zyklus (1200 ms)
  And das konfigurierte Intervall
```

### AK11: Polling auf jeder Verbindungsart

```gherkin
Scenario: Polling läuft auch in der Simulation
  Given der Server läuft mit einer Simulationsverbindung, auf der kein Modul antwortet
  And die Kanalliste dieser Spec mit Default-Intervall
  When der erste Poll-Zyklus abgelaufen ist
  Then wurde jeder Kanal der Kanalliste abgefragt
  And jeder Kanal ist als veraltet mit "kein Wert" markiert

Scenario: Intervall 0 auf dem Server
  Given der Server ist mit Poll-Intervall 0 gestartet
  When 5000 ms vergehen
  Then wurde keine Poll-Abfrage gesendet
  And Setzen über POST /api/channel/{addr}/{sub} funktioniert wie in Spec 0003
```

### AK12: Live-Beweis am echten Gerät (manuell, offen bis zur Laborsession)

Gegeben der Server läuft mit `--connection tcp` gegen das reale c't-Lab
mit der Kanalliste aus diesem Dokument
Wenn die Übersicht im Browser geöffnet ist
Dann zeigen alle Kanäle Werte, die sich mit den Messpunkten unten
decken. Ein vom Bus getrenntes bzw. nicht antwortendes Modul wird als
veraltet markiert, die anderen laufen weiter. Ein Setzen der
DDS-Frequenz im Bedienfeld funktioniert während des Pollings wie in
Spec 0003.

Bis AK12 erbracht ist, bleibt der Status höchstens "In Arbeit". Die
Messpunkte unten sind Voraussetzung.

## Explizit außerhalb des Scopes

- Jedes Schreiben über das bestehende DDS-Frequenz-Bedienfeld hinaus
  (keine neuen Stellglieder, keine DCG-Sollwerte aus der Übersicht)
- Dynamische Kanal-Erkennung, IDN-basierte Auswahl der Kanalliste,
  Laufzeit-Änderung der Kanalliste
- Ableiten der DIV-Einheit aus RNG zur Laufzeit. Das wird eine eigene,
  spätere Spec, hier wird RNG nur angezeigt.
- ESDM-Read-Model "Kanalübersicht". Nach der Laborsession neu bewerten.
- Sonderbehandlung des DIV-Overload-Werts `-99999` (unverifiziert, siehe
  Messpunkte; wird bis dahin wie jeder andere Wert angezeigt)
- `ALL?`-Sammelabfragen, Trigger-Masken (TRM/TRT/TRG), Kalibrier- und
  EEPROM-Kanäle (OFS/SCL/OPT/RAW)
- Exponentieller Backoff, Totband/Hysterese für "Änderung"
- Push unaufgeforderter Zeilen (z.B. Panel-Bedienung) über `/ws`, siehe AK9
- Aufzeichnung/Persistenz der Werte (CLAUDE.md "Nächste Schritte" Schritt 6)
- Tailwind und Layout-Feinschliff über eine einfache Übersicht hinaus

## Entschiedene Fragen (Betreiber, 2026-10-06)

1. **Kanalliste:** Der Vorschlag mit 13 Kanälen bleibt (siehe
   "Kanalliste für diese Spec").
2. **Ablage:** Die Kanalliste ist Server-Konfiguration. Es gibt kein
   ESDM-Read-Model, das wird nach der Laborsession neu bewertet.
3. **DIV 0:** Die Einheit wird statisch als "abhängig von RNG" geführt.
   Die Ableitung aus RNG wird eine eigene, spätere Spec.
4. **Verbindungsarten:** Das Polling läuft auf jeder Verbindung, auch in
   der Simulation (AK11). Intervall 0 bedeutet "aus" (AK2).
5. **Zyklus länger als das Intervall:** Zyklen überlappen nicht,
   verpasste Takte werden nicht nachgeholt, der nächste Zyklus startet
   sofort. Pro Episode gibt es eine Warnung, die gemessene Zykluszeit ist
   abrufbar (AK10).
6. **Änderungsfilter:** Er gilt nur für die Kanalliste. Quittungen
   wertet ausschließlich der Setz-Vorgang aus. Unaufgeforderte Zeilen
   werden geloggt und verworfen (AK9).
7. **AK6:** Die Setz-Sequenz (Setzen, Quittung, Rücklesen) ist im
   Lab-Actor atomar.
8. **AK7:** Der Snapshot für neue Clients bleibt.

## Offene Messpunkte für die nächste Laborsession

Voraussetzung: `octlab-server` ist **gestoppt**, solange mit `xport_probe`
bzw. `dds_probe` gemessen wird (nur eine XPort-Session, siehe CLAUDE.md).
Den Ausgangszustand jedes Stellglieds vorher notieren und danach
wiederherstellen.

1. **DIV-Firmware 3.10 vs. Doku 3.04:** Antworten die Subkanäle 0, 1, 2,
   10, 11 und 19 auf `?`, und in welchem Format? `1:IDN?`-Text für die
   Dokumentation festhalten.
2. **DIV RNG:** Wert von `1:19?` im aktuellen Zustand. Zusammenhang
   RNG-Code ↔ Panel-Anzeige (Bereich, V/A, AC/DC) für mindestens einen
   DC-Spannungsbereich prüfen. Ändert Panel-Bedienung RNG, und kommt dann
   eine unaufgeforderte Zeile?
3. **DIV Overload:** Liefert ein übersteuerter Eingang wirklich `-99999`?
   Nur mit einer sicheren Quelle im kleinsten Bereich prüfen, sonst
   auslassen.
4. **DIV Nachkommastellen/Rauschen:** 20 Abfragen von `1:0?` an offenen
   Klemmen. Wie viele Stellen kommen, wie stark schwankt der Wert? (Das
   bestimmt die Push-Rate nach AK7.)
5. **ADA-IO AD16 (10..17):** Antworten alle acht Kanäle? Einheit V und
   Bereich −10..+10 mit einer bekannten Spannung (z.B. DCG-Ausgang) an
   einem Kanal plausibilisieren.
6. **ADA-IO AD10 (0..7):** Antworten sie? Verhalten mit angeschlossenem
   LCD/Panel (Forenthread).
7. **ADA-IO DA12 (20..27):** Liefert `0:20?` etwas, und ist das der
   Sollwert?
8. **ADA-IO IO32 (30..37):** Welche Port-Subkanäle antworten (Erwartung
   30..33)?
9. **DCG (2):** `2:10?`, `2:11?`, `2:0?`, `2:1?`, `2:233?`. Antwortet
   jeder, und stimmen U Ist/U Soll mit der Panel-Anzeige überein? Damit
   ist auch der Widerspruch Tabelle (10/11) vs. Beispiel (2/3) aus [S2010]
   geklärt. Nur lesen, keine Sollwerte ändern.
10. **DDS (4):** `4:1?` (LVL, Einheit mVeff gegen Panel-Anzeige), `4:4?`,
    `4:20?`. Antworten `4:10?..4:12?` (TRMSC bestückt?)?
11. **Antwortzeiten:** Für je einen Kanal pro Modul die Zeit vom Senden
    bis zum Eintreffen der Antwortzeile messen, mit z.B. 20 Wiederholungen
    und Min/Max. Daraus folgt, ob 13 Kanäle in 1 s passen und wie viel
    Reserve bleibt, wenn ein Kanal 500 ms im Timeout hängt.
12. **Verschachtelte Abfragen:** Antworten zwei Module korrekt, wenn die
    zweite Abfrage gesendet wird, bevor die erste Antwort da ist (z.B.
    `1:0?` direkt gefolgt von `2:10?`)? Das entscheidet, ob Abfragen
    seriell laufen müssen oder überlappen dürfen.
13. **Nicht antwortendes Modul:** Wie verhält sich der Bus, wenn ein
    Modul aus der Kette genommen bzw. stromlos ist? Laufen Abfragen an
    die anderen Module weiter? (Die OptoBus-Kette leitet durch; ob das bei
    stromlosem Modul gilt, ist offen.)
