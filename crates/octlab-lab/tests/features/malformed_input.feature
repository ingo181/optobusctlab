Feature: Umgang mit ungültigen Empfangszeilen

  Als steuerndes System möchte ich, dass Zeilen ohne gültiges
  c't-Lab-Nachrichtenformat (z.B. Zeilenrauschen oder von der Hardware
  zurückgespiegelte eigene Befehle) den Betrieb nicht stören – weder Absturz
  noch fälschlich aufgelöste Anfragen. Echo ist dabei nur einer von mehreren
  möglichen Fällen ungültiger Eingabezeilen, keine Sonderbehandlung.

  Scenario: Eine ungültige Zeile wird übersprungen, die folgende gültige Antwort kommt trotzdem an
    Given ein simuliertes Modul an Adresse 0
    And das Modul sendet eine Zeile ohne gültiges Nachrichtenformat "*:IDN?"
    And das Modul antwortet auf die nächste Anfrage mit "#0:0=1.23456"
    When ich Subkanal 0 an Adresse 0 abfrage
    Then erhalte ich den Wert 1.23456

  Scenario: Nur eine ungültige Zeile führt zu einem sauberen Timeout, nicht zu einer falschen Auflösung
    Given ein simuliertes Modul an Adresse 0
    And das Modul sendet eine Zeile ohne gültiges Nachrichtenformat "*:IDN?"
    When ich Subkanal 0 an Adresse 0 abfrage
    Then läuft die Abfrage in einen Timeout
