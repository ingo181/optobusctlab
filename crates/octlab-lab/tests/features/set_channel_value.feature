Feature: Kanalwert setzen mit Quittung

  Als Bediener möchte ich einen Kanalwert (z.B. die DDS-Frequenz) setzen
  und eine belastbare Quittung erhalten, damit ich weiß, ob die Anlage
  den Befehl angenommen hat - wohl wissend, dass der tatsächlich
  eingestellte Wert nur per Rücklesen feststellbar ist (Spec 0003).

  Scenario: Anlage quittiert mit OK
    Given ein simuliertes Modul an Adresse 4
    And das Modul quittiert das nächste Kommando mit "#4:255=0 [OK]"
    When ich Subkanal 0 an Adresse 4 auf den Wert 2500 setze
    Then wurde das Kommando "4:0=2500!" gesendet
    And meldet das Setzen Erfolg mit Status "OK"

  Scenario: Anlage lehnt ab
    Given ein simuliertes Modul an Adresse 4
    And das Modul quittiert das nächste Kommando mit "#4:255=5 [PARERR]"
    When ich Subkanal 0 an Adresse 4 auf den Wert 99999999 setze
    Then meldet das Setzen Ablehnung mit Status "PARERR"

  Scenario: Anlage antwortet nicht
    Given ein simuliertes Modul an Adresse 4
    When ich Subkanal 0 an Adresse 4 auf den Wert 2500 setze
    Then meldet das Setzen keine Antwort
