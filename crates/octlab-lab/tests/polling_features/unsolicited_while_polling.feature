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
