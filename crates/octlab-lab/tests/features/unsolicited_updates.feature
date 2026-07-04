Feature: Unaufgeforderte Messwerte

  Module können Werte senden, ohne dass eine Abfrage vorausging (z.B. durch
  Bedienung am PM8-Panel oder einen Hardware-Trigger). Abonnenten des
  Live-Streams sollen diese Werte trotzdem sehen.

  Scenario: Ein Abonnent empfängt einen unaufgefordert gesendeten Wert
    Given ein simuliertes Modul an Adresse 2
    And das Modul sendet unaufgefordert "#2:1=42.0"
    When ich die Live-Updates des Labs abonniere
    Then empfange ich einen Wert von 42.0
