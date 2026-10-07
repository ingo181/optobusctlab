Feature: Verbindungsverlust zum XPort (Spec 0008)

  Bricht die Verbindung im Betrieb ab, liest der Lab-Actor nicht in einer
  Dauerschleife weiter, markiert die Kanalliste als veraltet, beantwortet
  Aufträge sofort mit Fehler und verbindet sich mit Backoff neu (1, 2, 4,
  8, 16, dann alle 30 s, gezählt ab dem Ende des vorigen Versuchs - Annahme,
  unverifiziert). Die Zeit läuft virtuell; der Simulator nimmt einen
  Verbindungsversuch ohne Verzögerung an oder lehnt ihn ab.

  Scenario: AK1 - Keine Dauerschleife nach Verbindungsende
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    And die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    And 20000 ms vergehen
    Then wurde die getrennte Verbindung höchstens einmal gelesen, beim Abbruch selbst

  Scenario: AK2 - Alle Kanäle werden veraltet, Werte bleiben
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    And ich abonniere den Änderungsstrom
    When 1 Poll-Zyklus abgelaufen ist
    And die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    Then ist jeder Kanal als veraltet markiert und behält seinen Wert
    And meldete der Änderungsstrom für "1:0" zuletzt "0.0022 veraltet"
    And meldete der Änderungsstrom für "4:0" zuletzt "1000.0 veraltet"

  Scenario: AK3 - Setzen ohne Verbindung scheitert sofort und wird nie nachgesendet
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    And die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    And ich "4:0" auf 2500 setze und zurücklese
    Then meldet die Setz-Sequenz "keine Verbindung" ohne Wartezeit
    When die Gegenseite wieder Verbindungen annimmt
    And die Verbindung wiederhergestellt ist
    And 3000 ms vergehen
    Then wurde "4:0=2500!" insgesamt 0 Mal gesendet
    And hat der Kanal "4:0" den Wert 1000.0

  # Jedes Modul antwortet nach 100 ms: Setz-Kommando bei 0 ms, Quittung bei
  # 100 ms, Rücklese-Abfrage bei 100 ms, Rücklesewert bei 200 ms.
  Scenario: AK3 - Abbruch nach dem Senden, vor der Quittung
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    And jedes Modul antwortet erst nach 100 ms
    When 1 Poll-Zyklus abgelaufen ist
    And ich "4:0" auf 2500 setze und zurücklese, während die Verbindung nach 50 ms abbricht
    Then meldet die Setz-Sequenz Verbindungsverlust ohne Quittung
    When die Verbindung wiederhergestellt ist
    And 3000 ms vergehen
    Then wurde "4:0=2500!" insgesamt 1 Mal gesendet
    And hat der Kanal "4:0" den Wert 2500.0

  Scenario: AK3 - Abbruch nach der Quittung, vor dem Rücklesewert
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    And jedes Modul antwortet erst nach 100 ms
    When 1 Poll-Zyklus abgelaufen ist
    And ich "4:0" auf 2500 setze und zurücklese, während die Verbindung nach 150 ms abbricht
    Then meldet die Setz-Sequenz Verbindungsverlust mit Quittung "OK"
    When die Verbindung wiederhergestellt ist
    And 3000 ms vergehen
    Then wurde "4:0=2500!" insgesamt 1 Mal gesendet

  Scenario: AK4 - Wiederverbindung mit Backoff bis zum Maximum
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    And die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    And 100000 ms vergehen
    Then erfolgten Verbindungsversuche bei "1, 3, 7, 15, 31, 61, 91" s nach dem Abbruch
    And wurde vor jedem Verbindungsversuch die vorige Session geschlossen

  Scenario: AK5 - Wiederaufnahme, und der Backoff beginnt danach wieder bei 1 s
    Given die Module liefern "1:0=0.0022, 4:0=1000.0"
    And die Kanalliste "1:0, 4:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    And die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    And 5000 ms vergehen
    And die Gegenseite wieder Verbindungen annimmt
    And die Verbindung wiederhergestellt ist
    And 1 Poll-Zyklus abgelaufen ist
    Then ist der Kanal "1:0" nicht als veraltet markiert
    And ist der Kanal "4:0" nicht als veraltet markiert
    When die Gegenseite die Verbindung abbricht
    And 3000 ms vergehen
    Then der erste Verbindungsversuch nach dem Abbruch erfolgte nach 1 s

  Scenario: AK6 - Verbindungszustand und Zahl der Versuche
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    Then ist der Verbindungszustand "verbunden" mit 0 Versuchen
    When die Gegenseite keine Verbindung mehr annimmt
    And die Gegenseite die Verbindung abbricht
    And 8000 ms vergehen
    Then ist der Verbindungszustand "getrennt" mit 3 Versuchen
    When die Gegenseite wieder Verbindungen annimmt
    And die Verbindung wiederhergestellt ist
    Then ist der Verbindungszustand "verbunden" mit 0 Versuchen

  Scenario: AK8 - Fail-fast beim Start bleibt
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And die Gegenseite nimmt keine Verbindung an
    When das Lab gestartet wird
    And 10000 ms vergehen
    Then ist der Start mit einem Verbindungsfehler gescheitert
    And gab es insgesamt genau 1 Verbindungsversuch
