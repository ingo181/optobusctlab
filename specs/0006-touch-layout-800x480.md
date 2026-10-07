# 0006 – Touch-Layout 800x480

Status: In Arbeit

## Kontext

Zielbild laut CLAUDE.md ist ein Raspberry Pi mit 7"-Touch-Display auf dem
c't-Lab-Gehäuse, Kiosk-Browser, Bildschirm 800x480. Das heutige Frontend
ist eine einzige, senkrecht gestapelte Seite: Gauge (DIV), DDS-Bedienfeld,
Kanalübersicht (Spec 0005).

**Beobachtung (2026-10-07, Headless-Firefox gegen `fake_xport --mute 2`,
nicht am echten Gerät):**

- Bei 800x480 ist **ohne Scrollen nur Titel, Gauge und der obere Teil des
  DDS-Bedienfelds** sichtbar. Schon die Zeile "noch nichts gesetzt" liegt
  unterhalb von 480 px. Die Kanalübersicht ist gar nicht zu sehen.
- Die Seite braucht etwa 1180 px Höhe, also rund 2,5 Bildschirmhöhen. Die
  Tabelle beginnt bei etwa 700 px. Den größten Teil nimmt das Gauge ein
  (etwa 360 px).
- **Horizontal passt alles:** Die Tabelle ist bei 800 px Breite etwa
  535 px breit, keine Spalte wird abgeschnitten.
- Touch-Ziele heute (aus dem Screenshot abgelesen, ca.): Eingabefeld und
  Button "Setzen" sind jeweils etwa 36 px hoch, also unter den etwa 44 px,
  die für Finger-Bedienung üblich sind.

Diese Spec macht jede Ansicht bei 800x480 ohne Scrollen nutzbar, indem
die drei Bereiche auf Tabs verteilt werden.

**Entschieden (Vorgabe Betreiber, 2026-10-07):**

- Tab-Navigation mit drei Ansichten: **Gauge**, **Übersicht**,
  **DDS-Steuerung**.
- Jede Ansicht ist bei 800x480 ohne Scrollen nutzbar.
- Touch-Ziele mindestens ca. 44 px.
- Nachweis über Headless-Screenshots (Hilfsmittel: `tools/screenshot/`);
  das Verhalten am echten Touch-Gerät ist ein offener Messpunkt.
- Start-Tab nach dem Laden: **Gauge**.
- Tabs gibt es in **allen** Fenstergrößen (ein Layout, ein Code-Pfad).
- In der DDS-Ansicht wird **Platz für ein eigenes Ziffernfeld reserviert**;
  das Ziffernfeld selbst kommt mit Spec 0007. Das Eingabefeld bleibt
  (Desktop-Bedienung mit Tastatur).
- Umsetzung im **bestehenden handgeschriebenen CSS**, ohne Tailwind.

**Unverändert bleiben:** der `/ws`-Vertrag und die HTTP-Endpunkte (Spec
0005), das Verhalten des DDS-Bedienfelds (Spec 0003: nur der
Rücklesewert wird als bestätigte Frequenz gezeigt), die Anzeige-Logik der
Übersicht (Spec 0005: "kein Wert", "veraltet").

## Akzeptanzkriterien

Die Screenshot-Kriterien werden mit Headless-Firefox bei einer Fenstergröße
von genau 800x480 geprüft, gegen `fake_xport --mute 2` (damit "veraltet"
sichtbar ist), mit dem Hilfsmittel aus `tools/screenshot/` (siehe dessen
README). "Ohne Scrollen sichtbar" heißt: vollständig innerhalb des
800x480-Ausschnitts, nichts abgeschnitten.

### AK1: Tab-Leiste in jeder Ansicht

Gegeben das Frontend bei 800x480
Wenn eine der drei Ansichten aktiv ist
Dann ist die Tab-Leiste mit den Einträgen "Gauge", "Übersicht" und
"DDS-Steuerung" vollständig sichtbar und der aktive Tab ist erkennbar
hervorgehoben

### AK1a: Start-Tab ist Gauge

Gegeben das Frontend wird (neu) geladen
Wenn noch kein Tab gewählt wurde
Dann ist der Tab "Gauge" aktiv und die Gauge-Ansicht sichtbar

### AK2: Gauge-Ansicht ohne Scrollen

Gegeben das Frontend bei 800x480 mit aktivem Tab "Gauge"
Wenn DIV-Messwerte eintreffen
Dann sind Skala mit Beschriftung, Zeiger, Zahlenwert und
Kanalbeschriftung ohne Scrollen sichtbar

### AK3: Übersicht ohne Scrollen

Gegeben das Frontend bei 800x480 mit aktivem Tab "Übersicht" und den 12
Kanälen aus Spec 0005
Wenn die Kanalliste geladen ist
Dann sind Tabellenkopf, alle 12 Zeilen (Name, Adresse:Subkanal, Wert,
Einheit, Status) und der Hinweis zur Verifikation ohne Scrollen sichtbar,
und veraltete Kanäle sind weiterhin als "veraltet" erkennbar

Anmerkung: Tabellenzeilen sind keine Touch-Ziele (nicht bedienbar), für
sie gilt die 44-px-Grenze aus AK5 nicht.

### AK4: DDS-Steuerung ohne Scrollen, mit reserviertem Platz fürs Ziffernfeld

Gegeben das Frontend bei 800x480 mit aktivem Tab "DDS-Steuerung"
Wenn ein Setz-Vorgang abgeschlossen ist (Erfolg oder Fehler)
Dann sind Eingabefeld, Button "Setzen", bestätigte Frequenz und ein
Hinweistext (z.B. "keine Quittung (Timeout), Rücklesewert liegt vor")
gleichzeitig ohne Scrollen sichtbar, und daneben oder darunter ist ein
freier Bereich für das Ziffernfeld aus Spec 0007 reserviert, ebenfalls
ohne Scrollen sichtbar

Vorschlag für die Größe des reservierten Bereichs (endgültig in Spec
0007): mindestens 3 Spalten und 4 Zeilen à 44 px plus Abstände, also
etwa 160 x 200 px. Bis 0007 bleibt der Bereich leer.

### AK5: Touch-Ziele mindestens ca. 44 px

Gegeben das Frontend bei 800x480
Wenn die bedienbaren Elemente vermessen werden (Tabs, Eingabefeld, Button
"Setzen")
Dann ist jedes mindestens ca. 44 px hoch und breit (geprüft am Screenshot
bzw. an den berechneten Elementgrößen)

### AK6: Tab-Wechsel verliert keinen Zustand

Gegeben eine bestätigte DDS-Frequenz im Tab "DDS-Steuerung" und
laufende Messwerte
Wenn zu "Übersicht" und zurück gewechselt wird
Dann zeigt die DDS-Steuerung weiterhin dieselbe bestätigte Frequenz, und
Übersicht und Gauge zeigen nach dem Wechsel sofort den aktuellen Stand
(keine neue WebSocket-Verbindung, kein Warten auf die nächste Änderung)

### AK7: Größere Fenster bleiben nutzbar

Gegeben das Frontend in einem großen Fenster (z.B. 1000x1300, Desktop-App
oder Browser am PC)
Wenn die drei Tabs nacheinander aufgerufen werden
Dann ist jede Ansicht vollständig und ohne Überlappungen dargestellt

### AK8: Touch-Bedienung am echten Gerät (manuell, offen)

Gegeben der Pi mit 7"-Touch-Display im Kiosk-Browser gegen die reale Anlage
Wenn mit dem Finger zwischen den Tabs gewechselt und eine DDS-Frequenz
eingegeben und gesetzt wird
Dann gelingt jede Bedienung ohne Fehlgriffe, und jede Ansicht ist ohne
Scrollen nutzbar; Befunde siehe Messpunkte unten

Bis AK8 erbracht ist, bleibt der Status höchstens "In Arbeit".

## Prüfart je AK und Rot-Nachweis

Prüfart: **Rust** = Host-Test in `apps/web/src/tabs.rs` (läuft unter
`cargo test --workspace`); **Screenshot** = Sichtung eines PNG aus
`tools/screenshot/shot.sh --size <BxH> --tab <fragment> --mute 2`;
**von Hand** = Bedienung durch den Betreiber. Komponenten, Klicks und
Layout sind in Rust nicht testbar (Leptos CSR, keine Browser-Tests, keine
neue Abhängigkeit - Entscheidung 2026-10-07).

Die Tab-Auswahl beim Laden läuft über das URL-Fragment (`#gauge`,
`#uebersicht`, `#dds`), nur beim Laden gelesen; Klicks schreiben es
nicht; unbekanntes oder fehlendes Fragment -> Gauge (Entscheidung
2026-10-07, Host-Test in `tabs.rs`). Damit kann das Screenshot-Werkzeug
jeden Tab ansteuern, ohne zu klicken.

**Rot-Nachweis vor der Umsetzung (2026-10-07):** Host-Tests in `tabs.rs`
gegen eine leere Hülle: 6 von 6 rot. Screenshots je Fragment bei 800x480
und 1000x1300 (`--mute 2`): Die App ignoriert das Fragment, alle zeigen
dieselbe Einzelseite mit Titelzeile, Gauge, DDS-Bedienfeld und Übersicht.

| AK | Prüfart | Stand vor der Umsetzung |
|----|---------|-------------------------|
| AK1 Tab-Leiste | Rust (Reihenfolge, Beschriftung) + Screenshot je Tab | rot: keine Tab-Leiste |
| AK1a Start-Tab Gauge | Rust (Default, fehlendes/unbekanntes Fragment) + Screenshot ohne Fragment | rot: Hülle; keine Tabs |
| AK2 Gauge ohne Scrollen | Screenshot 800x480 `#gauge` | rot: Gauge passt zwar ganz in 800x480, aber ohne Tab-Leiste (AK1) |
| AK3 Übersicht ohne Scrollen | Screenshot 800x480 `#uebersicht` | rot: Übersicht bei 800x480 gar nicht sichtbar |
| AK4 DDS-Steuerung | Screenshot 800x480 `#dds` für Eingabe, Button, bestätigte Frequenz, reservierten Bereich; **von Hand** für den Hinweistext nach einem Setz-Vorgang (das Bedienfeld zeigt nur Ergebnisse eigener Klicks, der Screenshot kann nicht klicken) | rot: Bedienfeld bei 480 px angeschnitten, kein reservierter Bereich |
| AK5 Touch-Ziele ca. 44 px | Screenshot, Pixel ausgemessen (Tabs, Eingabefeld, "Setzen") | rot: Eingabefeld und "Setzen" ca. 36 px hoch; keine Tabs |
| AK6 Kein Zustandsverlust | **von Hand** im Desktop-Browser (Wert eintippen und setzen, Tab wechseln, zurück); abgesichert durch die Bauweise (alle Ansichten bleiben gemountet) | nicht anwendbar (keine Tabs) |
| AK7 Große Fenster | Screenshot 1000x1300 je Tab | rot: alles sichtbar, aber keine Tabs |
| AK8 Echtes Gerät | **von Hand** im Labor | offen |

**Ergebnisse während der Umsetzung:**

- **AK6 Handtest bestanden** (2026-10-07, Betreiber Ingo, Desktop-Browser
  gegen `fake_xport` ohne `--mute`, Stand nach Schritt 2, Commit
  `1e8f7e2`). Alle vier Testschritte verhielten sich wie beschrieben,
  keine Abweichungen:
  1. Wert 2500 gesetzt, die bestätigte Frequenz erschien.
  2. 3000 getippt und NICHT gesetzt, dann über Übersicht und Gauge zurück
     zu DDS: Das Eingabefeld zeigte 3000, die bestätigte Frequenz 2500.
  3. Die Übersicht zeigte "DDS Frequenz" 2500 mit aktuellen Werten.
  4. Direktes Laden mit `#dds`, danach Wechsel zu Gauge: Das Gauge
     erschien in voller Größe und lief live (damit auch die Prüfung "Gauge
     im ausgeblendeten Zustand", siehe unten, von Hand bestätigt).

- **Screenshot-Befunde nach Schritt 4** (2026-10-07, Stand `41ac399`,
  `tools/screenshot/shot.sh --size <BxH> --tab <fragment> --mute 2`).
  Maße im gerenderten PNG ausgemessen (Headless-Firefox, 1 Bildpixel =
  1 CSS-Pixel), nicht nur aus dem CSS abgeleitet:

  | Element (800x480) | gemessen | Lage |
  |---|---|---|
  | Tab-Leiste | 52 px hoch | y 0-51 |
  | Tab "Gauge" | 81 x 44 px | x 140-220, y 4-47 |
  | Tab "Übersicht" | 104 x 44 px | x 229-332, y 4-47 |
  | Tab "DDS-Steuerung" | 148 x 44 px | x 341-488, y 4-47 |
  | Eingabefeld | 266 x 44 px | x 129-394, y 77-120 |
  | Button "Setzen" | 92 x 44 px | x 403-494, y 77-120 |

  Abstände: je 8 px zwischen den Tabs und zwischen Eingabefeld und
  "Setzen" - kein Touch-Ziel berührt ein anderes. Bei 1000x1300 identische
  Größen (Inhalt nur horizontal verschoben).

  | AK | Befund |
  |---|---|
  | AK1 | erfüllt: Tab-Leiste in jeder Ansicht, aktiver Tab hell hinterlegt (Rust-Tests Reihenfolge/Beschriftung grün) |
  | AK1a | erfüllt: ohne Fragment ist Gauge aktiv (Rust-Tests grün, Screenshot) |
  | AK2 | erfüllt: Gauge vollständig sichtbar, unterste Inhaltszeile y = 438 (inkl. Kartenschatten), 41 px Rand |
  | AK3 | erfüllt: Kopf, alle 12 Zeilen, "veraltet" und Verifikationshinweis sichtbar, unterste Inhaltszeile y = 448, 31 px Rand; Schrift 15,2 px |
  | AK4 | per Screenshot erfüllt: Eingabefeld, "Setzen", "noch nichts gesetzt", Beschriftung und der reservierte Bereich (160 x 200 px, gestrichelt) sichtbar, unterste Inhaltszeile y = 259, 220 px frei. Hinweistext nach einem Setz-Vorgang: per Handtest erfüllt (siehe "AK4 Handtest" unten) |
  | AK5 | erfüllt: alle Touch-Ziele mindestens 44 x 44 px (Tabelle oben) |
  | AK6 | erfüllt (Handtest, siehe oben) |
  | AK7 | erfüllt: bei 1000x1300 alle drei Ansichten vollständig, nichts abgeschnitten |
  | AK8 | offen (Labor) |

- **AK4 Handtest** (2026-10-07, Betreiber Ingo, Desktop-Browser gegen
  `fake_xport`, Screenshots beim Betreiber). Damit ist AK4 vollständig
  geprüft:
  1. Gültiger Wert 2500: bestätigte Frequenz erscheint, ohne Hinweis. OK.
  2. Hinweistext "keine Antwort vom Modul (Timeout)" (`fake_xport --mute
     4`): lesbar, keine Überlappung, der reservierte Bereich bleibt frei.
     OK.
  3. Ungültige Eingabe "abc": Rahmen des Eingabefelds rot, kein
     Hinweistext, kein Layoutsprung. OK.
  4. Langer Wert: **Mangel gefunden** - bei einer langen Ziffernfolge lief
     "Bestätigt: ... Hz" über den Kartenrand in den reservierten Bereich
     (eine Ziffernfolge hat keine Umbruchstelle; `fake_xport` klemmt nicht
     und liest den Wert ungekürzt zurück). **Behoben mit Fix A**
     (`9c15ff7`, `overflow-wrap: anywhere` für die bestätigte Frequenz und
     den Hinweistext). Nachweis: statische Seite mit dem echten Stylesheet
     und dem Markup der DDS-Ansicht bei 800x480 (1e40 lief vorher über,
     bricht nachher innerhalb der Karte um) und Handtest in der echten App
     mit 1e40 durch den Betreiber: Umbruch innerhalb der Karte, nichts
     überlappt. **Bekannte Grenze:** Extreme Werte (z.B. 1e300) machen die
     Karte höher als 480 px (Scrollen). Eine Formatierung der Anzeige (Fix
     B) ist zurückgestellt, weil die echte DDS-Firmware auf 999999,8 Hz
     klemmt (Spec 0003, verifiziert) - solche Werte kommen nur vom
     Simulator.

  **Stand Spec 0006:** AK1-AK7 geprüft; offen bleibt nur AK8 (Labor).

**Gauge im ausgeblendeten Zustand:** Das Gauge darf nicht mit falscher
Größe erscheinen, wenn es ausgeblendet gerendert wurde und erst später
sichtbar wird. Das Screenshot-Werkzeug kann keinen Tab-Wechsel auslösen;
per Screenshot geprüft wird deshalb das Laden direkt in jedem Tab
(`#gauge`, `#uebersicht`, `#dds`). Der Wechsel zurück zum Gauge nach dem
Laden in einem anderen Tab ist Teil des Handtests AK6.

## Explizit außerhalb des Scopes

- Neue Instrumente, neue Kanäle oder Stellglieder (nur Neuanordnung des
  Bestehenden)
- Gesten wie Wischen zwischen Tabs, Animationen
- Hochformat, andere Auflösungen als 800x480 als Pflichtziel (nur AK7 als
  Regressionsschutz)
- Kiosk-Setup des Pi selbst (Autostart, Browser-Konfiguration)
- Das Ziffernfeld selbst - eigene Spec 0007; hier wird nur Platz
  reserviert (AK4)
- Tailwind - 0006 bleibt beim handgeschriebenen `apps/web/styles.css`
  (Stand 2026-10-07: eine Datei, 203 Zeilen, etwa 32 Regelblöcke, keine
  Inline-Styles; Grundlage für eine spätere, getrennte Bewertung)
- Änderungen an `/ws`, HTTP-Endpunkten oder der Lab-Ebene

## Alternativen (betrachtet, nicht gewählt)

- **Zwei-Spalten-Layout ohne Tabs** (Gauge links, Übersicht rechts,
  DDS-Steuerung darunter oder in der linken Spalte): Gauge (heute etwa
  465 px Kartenbreite) und Tabelle (etwa 535 px) ergeben zusammen gut
  1000 px, also mehr als 800. Es ginge nur mit verkleinertem Gauge
  und/oder schmalerer Tabelle (z.B. ohne Spalte "Adr:Sub"). Vorteil: alles
  auf einen Blick, keine Navigation. Nachteil: 12 Tabellenzeilen plus
  DDS-Bedienfeld mit 44-px-Zielen werden in 480 px Höhe knapp, das Gauge
  würde sehr klein.
- **Eine Seite, aber kompakter** (kleineres Gauge, engere Abstände): löst
  das Grundproblem nicht. Gauge plus 12 Zeilen plus Bedienfeld passen
  nicht in 480 px Höhe, ohne unter die Lesbarkeit zu gehen.
- **Wischgesten statt Tab-Leiste:** schwer auffindbar und fehleranfälliger
  auf resistiven bzw. ungenauen Touchscreens; Tabs bleiben auch mit der
  Maus bedienbar (Desktop-App).

## Entschiedene Fragen (Betreiber, 2026-10-07)

1. **Bildschirmtastatur:** Eigenes Ziffernfeld in der DDS-Ansicht statt
   System-Bildschirmtastatur - als eigene **Spec 0007** nach dieser. In
   0006 wird nur Platz dafür reserviert (AK4); das Eingabefeld bleibt für
   die Desktop-Bedienung.
2. **Start-Tab:** Gauge (AK1a). Ob der zuletzt gewählte Tab ein Neuladen
   überlebt, ist nicht Teil dieser Spec.
3. **Tabs auch auf großen Fenstern:** ja, ein Layout für alle Größen
   (AK7).
4. **Tailwind:** nicht in 0006, Umsetzung im bestehenden CSS (siehe
   "Explizit außerhalb des Scopes"). Bewertung später und getrennt.
5. **Screenshot-Nachweis:** Das Hilfsmittel liegt im Repo unter
   `tools/screenshot/` (Skript, Hilfsseite, README; keine Binaries, keine
   neue Crate).

## Offene Messpunkte für die nächste Laborsession

1. **Tatsächliche Auflösung und Skalierung am Pi:** Liefert das Display im
   Kiosk-Browser wirklich einen Viewport von 800x480 CSS-Pixeln, oder
   skaliert der Browser (Device Pixel Ratio, Zoom)? Bleiben Browser-Leisten
   übrig, die Höhe kosten?
2. **Touch-Genauigkeit:** Treffen Finger zuverlässig Ziele von etwa 44 px
   (Tabs, "Setzen")? Gibt es Fehlgriffe zwischen benachbarten Tabs?
3. **Texteingabe:** Taucht im Kiosk-Browser beim Antippen des
   Eingabefelds trotzdem eine System-Bildschirmtastatur auf (verdeckt sie
   die Ansicht)? Relevant für Spec 0007 (eigenes Ziffernfeld).
4. **Lesbarkeit:** Sind Tabellenzeilen und Zahlenwerte aus Arbeitsabstand
   am Gehäuse gut lesbar (Schriftgröße, Kontrast der "veraltet"-Zeilen)?
