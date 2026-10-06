# 0006 – Touch-Layout 800x480

Status: Entwurf

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
- Nachweis über Headless-Screenshots; das Verhalten am echten Touch-Gerät
  ist ein offener Messpunkt.

**Unverändert bleiben:** der `/ws`-Vertrag und die HTTP-Endpunkte (Spec
0005), das Verhalten des DDS-Bedienfelds (Spec 0003: nur der
Rücklesewert wird als bestätigte Frequenz gezeigt), die Anzeige-Logik der
Übersicht (Spec 0005: "kein Wert", "veraltet").

## Akzeptanzkriterien

Die Screenshot-Kriterien werden mit Headless-Firefox bei einer Fenstergröße
von genau 800x480 geprüft, gegen `fake_xport --mute 2` (damit "veraltet"
sichtbar ist). "Ohne Scrollen sichtbar" heißt: vollständig innerhalb des
800x480-Ausschnitts, nichts abgeschnitten.

### AK1: Tab-Leiste in jeder Ansicht

Gegeben das Frontend bei 800x480
Wenn eine der drei Ansichten aktiv ist
Dann ist die Tab-Leiste mit den Einträgen "Gauge", "Übersicht" und
"DDS-Steuerung" vollständig sichtbar und der aktive Tab ist erkennbar
hervorgehoben

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

### AK4: DDS-Steuerung ohne Scrollen

Gegeben das Frontend bei 800x480 mit aktivem Tab "DDS-Steuerung"
Wenn ein Setz-Vorgang abgeschlossen ist (Erfolg oder Fehler)
Dann sind Eingabefeld, Button "Setzen", bestätigte Frequenz und ein
Hinweistext (z.B. "keine Quittung (Timeout), Rücklesewert liegt vor")
gleichzeitig ohne Scrollen sichtbar

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

## Explizit außerhalb des Scopes

- Neue Instrumente, neue Kanäle oder Stellglieder (nur Neuanordnung des
  Bestehenden)
- Gesten wie Wischen zwischen Tabs, Animationen
- Hochformat, andere Auflösungen als 800x480 als Pflichtziel (nur AK7 als
  Regressionsschutz)
- Kiosk-Setup des Pi selbst (Autostart, Browser-Konfiguration)
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

## Offene Fragen

1. **Bildschirmtastatur:** Am Kiosk gibt es keine physische Tastatur. Eine
   System-Bildschirmtastatur verdeckt bei 480 px Höhe einen großen Teil
   der DDS-Steuerung. Lieber ein eigenes Ziffernfeld in der Ansicht
   (0-9, Komma, Löschen, Setzen) statt des Systemfelds? Das würde AK4/AK5
   betreffen.
2. **Start-Tab:** Welcher Tab ist nach dem Laden aktiv (Vorschlag:
   Übersicht)? Soll der zuletzt gewählte Tab ein Neuladen überleben (z.B.
   per URL-Fragment)?
3. **Tabs auch auf großen Fenstern?** Vorschlag: ja, ein Layout für alle
   Größen (YAGNI, ein Code-Pfad). Alternative: Tabs nur unterhalb einer
   Bildschirmhöhe, darüber die heutige Einzelseite.
4. **Tailwind:** CLAUDE.md legt Tailwind 4 fest, "sobald die UI über ein
   einzelnes Instrument hinauswächst" - das ist mit Spec 0005 eingetreten.
   Führt diese Spec Tailwind ein, oder bleibt es bei handgeschriebenem
   `styles.css` und Tailwind kommt als eigener Schritt?
5. **Reproduzierbarer Screenshot-Nachweis:** Headless-Firefox löst seinen
   Screenshot beim `load`-Ereignis aus, also bevor die WASM-App gemountet
   ist. Für die Beobachtung oben lief deshalb eine Wegwerf-Hilfsseite
   (iframe plus absichtlich verzögerte Ressource), die nicht im Repo liegt.
   Soll ein solches Hilfsmittel ins Repo (z.B. als Rust-Example analog
   `fake_xport`), oder bleibt der Screenshot-Nachweis ein dokumentiertes
   manuelles Verfahren?

## Offene Messpunkte für die nächste Laborsession

1. **Tatsächliche Auflösung und Skalierung am Pi:** Liefert das Display im
   Kiosk-Browser wirklich einen Viewport von 800x480 CSS-Pixeln, oder
   skaliert der Browser (Device Pixel Ratio, Zoom)? Bleiben Browser-Leisten
   übrig, die Höhe kosten?
2. **Touch-Genauigkeit:** Treffen Finger zuverlässig Ziele von etwa 44 px
   (Tabs, "Setzen")? Gibt es Fehlgriffe zwischen benachbarten Tabs?
3. **Texteingabe:** Wie verhält sich die Frequenz-Eingabe ohne physische
   Tastatur (Bildschirmtastatur ja/nein, verdeckter Bereich)? Entscheidet
   offene Frage 1.
4. **Lesbarkeit:** Sind Tabellenzeilen und Zahlenwerte aus Arbeitsabstand
   am Gehäuse gut lesbar (Schriftgröße, Kontrast der "veraltet"-Zeilen)?
