Feature: Unaufgeforderte Zeilen während des Pollings (Spec 0005, AK9 Lab-Ebene)

  Auf Lab-Ebene verteilt subscribe() unaufgeforderte Zeilen weiterhin an
  alle Abonnenten (siehe unsolicited_updates.feature). Sie ändern aber den
  Kanalzustand der Übersicht nicht und erscheinen nicht im Änderungsstrom.

  Scenario: AK9 - Unaufgeforderter Wert wird verteilt, ändert aber den Kanalzustand nicht
    Given die Module liefern "4:0=1000.0"
    And die Kanalliste "4:0"
    And ein Poll-Intervall von 1000 ms
    And ich abonniere den Änderungsstrom
    And ich abonniere die Lab-Updates
    When der Kanal "4:0" einen Wert geliefert hat
    And das Modul unaufgefordert "#4:0=440.0" sendet
    Then empfängt der Lab-Abonnent den Wert 440.0
    And hat der Kanal "4:0" den Wert 1000.0
    And enthält der Änderungsstrom keine Meldung mit Wert 440.0

  Scenario: AK9 - Echo- oder Müllzeile während des Pollings
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 1000 ms
    When das Modul unaufgefordert "1:0?" sendet
    And 3000 ms vergehen
    Then wurde der Kanal "1:0" 3 Mal abgefragt
    And ist kein Kanal als veraltet markiert

  # Die Abfrage "1:0?" geht bei 0 ms raus, die Antwort kommt bei 100 ms.
  # Bei 50 ms treffen zwei Müllzeilen ein: eine, die wie eine Antwort auf
  # genau diesen Kanal aussieht, aber kein "#" hat, und eine mit "#", aber
  # unlesbarem Wert. Keine darf als Wert durchgehen (verfälschen), keine
  # darf das Warten auf die echte Antwort beenden (stehlen) - sonst wäre der
  # Kanal danach veraltet. Geprüft wird bei 150 ms, also nach der echten
  # Antwort und VOR dem nächsten Zyklus (1000 ms), der einen verfälschten
  # Wert sonst wieder überschreiben und den Fehler verdecken würde.
  Scenario: AK9 - Müllzeile mitten in einer Poll-Abfrage verfälscht nichts und stiehlt keine Antwort
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 1000 ms
    And jedes Modul antwortet erst nach 100 ms
    When 50 ms vergehen
    And das Modul unaufgefordert "1:0=99" sendet
    And das Modul unaufgefordert "#1:0=abc" sendet
    And 100 ms vergehen
    Then hat der Kanal "1:0" den Wert 0.0022
    And ist der Kanal "1:0" nicht als veraltet markiert
