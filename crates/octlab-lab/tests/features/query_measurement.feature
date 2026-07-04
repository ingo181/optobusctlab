Feature: Messwertabfrage vom c't-Lab

  Als steuerndes System möchte ich einen Messwert von einem Modul abfragen
  und dessen aktuellen Wert erhalten, damit ich Steuer- und
  Regelentscheidungen treffen kann.

  Scenario: Modul antwortet innerhalb des Timeouts
    Given ein simuliertes Modul an Adresse 0
    And das Modul antwortet auf die nächste Anfrage mit "#0:0=1.23456"
    When ich Subkanal 0 an Adresse 0 abfrage
    Then erhalte ich den Wert 1.23456

  Scenario: Modul antwortet nicht
    Given ein simuliertes Modul an Adresse 0
    When ich Subkanal 0 an Adresse 0 abfrage
    Then läuft die Abfrage in einen Timeout
