Feature: Setz-Sequenz während laufenden Pollings (Spec 0005, AK6)

  Setzen, Quittung und Rücklesen laufen im Lab-Actor als eine unteilbare
  Einheit und haben Vorrang vor Poll-Abfragen. Abweichend von Spec 0003
  wird auch ohne Quittung zurückgelesen (verbindlich ist nur das
  Rücklesen). Jedes Modul antwortet hier nach 100 ms; der Poll-Zyklus
  beginnt bei 0 ms mit "1:0?", um 100 ms folgt "2:10?" - die
  Setz-Anforderung bei 150 ms trifft also mitten in den Zyklus.

  Background:
    Given die Module liefern "1:0=0.0022, 2:10=12.0, 0:10=0.0021, 0:11=1.25, 4:0=1000.0"
    And die Kanalliste "1:0, 2:10, 0:10, 0:11, 4:0"
    And ein Poll-Intervall von 1000 ms
    And jedes Modul antwortet erst nach 100 ms

  Scenario: AK6 - Setz-Sequenz überholt anstehende Poll-Abfragen
    When 150 ms vergehen
    And ich "4:0" auf 2500 setze und zurücklese
    Then wurde zwischen Setz-Anforderung und "4:0=2500!" keine Abfrage gesendet
    And meldet die Setz-Sequenz Quittung "OK" und Rücklesewert 2500.0

  Scenario: AK6 - Keine Poll-Abfrage innerhalb der Setz-Sequenz
    When 150 ms vergehen
    And ich "4:0" auf 2500 setze und zurücklese
    Then folgt im Sende-Log auf "4:0=2500!" direkt "4:0?" nach 100 ms
    And meldet die Setz-Sequenz Quittung "OK" und Rücklesewert 2500.0
    And wird danach weiter gepollt

  Scenario: AK6 - Ohne Quittung wird trotzdem zurückgelesen
    Given das Modul an Adresse 4 quittiert nicht
    When 150 ms vergehen
    And ich "4:0" auf 2500 setze und zurücklese
    Then folgt im Sende-Log auf "4:0=2500!" direkt "4:0?" nach 500 ms
    And meldet die Setz-Sequenz keine Quittung und Rücklesewert 2500.0
    And wird danach weiter gepollt

  Scenario: AK6 - Ohne Quittung und ohne Rücklesen blockiert höchstens 500 + 500 ms
    Given das Modul an Adresse 4 antwortet nicht
    When 150 ms vergehen
    And ich "4:0" auf 2500 setze und zurücklese
    Then folgt im Sende-Log auf "4:0=2500!" direkt "4:0?" nach 500 ms
    And dauerte die Setz-Sequenz ab "4:0=2500!" genau 1000 ms
    And meldet die Setz-Sequenz keine Quittung und keinen Rücklesewert
    And wird danach weiter gepollt

  Scenario: AK6 - Rücklesewert erscheint sofort im Kanalzustand
    When 150 ms vergehen
    And ich "4:0" auf 2500 setze und zurücklese
    Then hat der Kanal "4:0" den Wert 2500.0
