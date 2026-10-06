Feature: Zyklisches Polling einer festen Kanalliste (Spec 0005)

  Der Lab-Actor fragt die Kanäle einer festen Liste zyklisch ab, merkt sich
  pro Kanal den letzten Wert und markiert Kanäle, die nicht antworten, als
  veraltet. Die Zeit läuft in diesen Szenarien virtuell (pausierte
  Tokio-Uhr), alle Zeitangaben sind deshalb exakt.

  Scenario: AK1 - Alle konfigurierten Kanäle liefern Werte
    Given die Module liefern "1:0=0.0022, 2:10=12.0, 4:0=1000.0"
    And die Kanalliste "1:0, 2:10, 4:0"
    And ein Poll-Intervall von 1000 ms
    When 1 Poll-Zyklus abgelaufen ist
    Then wurde jeder der Kanäle "1:0, 2:10, 4:0" genau einmal abgefragt
    And hat der Kanal "1:0" den Wert 0.0022
    And hat der Kanal "2:10" den Wert 12.0
    And hat der Kanal "4:0" den Wert 1000.0
    And ist kein Kanal als veraltet markiert

  Scenario: AK2 - Abweichendes Intervall
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 2000 ms
    When 6000 ms vergehen
    Then wurde der Kanal "1:0" 3 Mal abgefragt

  Scenario: AK2 - Default-Intervall
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And kein Poll-Intervall ist konfiguriert
    When 3000 ms vergehen
    Then wurde der Kanal "1:0" 3 Mal abgefragt

  Scenario: AK2 - Intervall 0 schaltet das Polling ab
    Given die Module liefern "1:0=0.0022"
    And die Kanalliste "1:0"
    And ein Poll-Intervall von 0 ms
    When 5000 ms vergehen
    Then wurde keine Abfrage gesendet
    And ist kein Kanal als veraltet markiert

  Scenario: AK3 - Ein Kanal timeoutet, die anderen bleiben aktuell
    Given die Module liefern "1:0=0.0022, 2:10=12.0, 4:0=1000.0"
    And die Kanalliste "1:0, 2:10, 4:0"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When 3 Poll-Zyklen abgelaufen sind
    Then ist der Kanal "2:10" als veraltet markiert
    And wurden die Kanäle "1:0, 4:0" je 3 Mal abgefragt
    And ist der Kanal "1:0" nicht als veraltet markiert
    And ist der Kanal "4:0" nicht als veraltet markiert

  Scenario: AK4 - Kein erneuter Versuch vor Ablauf des Backoffs
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When die Abfrage von "2:10" ins Timeout läuft
    Then wird "2:10" in den folgenden 5000 ms nicht erneut abgefragt

  Scenario: AK4 - Wiederaufnahme nach dem Backoff
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When die Abfrage von "2:10" ins Timeout läuft
    And das Modul an Adresse 2 wieder antwortet
    And der Kanal "2:10" wieder einen Wert geliefert hat
    Then hat der Kanal "2:10" den Wert 12.0
    And ist der Kanal "2:10" nicht als veraltet markiert
    And lag zwischen Timeout und erneuter Abfrage von "2:10" mindestens 5000 ms
    And wird "2:10" in den nächsten 3 Zyklen jeweils abgefragt

  Scenario: AK4 - Erneutes Timeout nach dem Backoff
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When 20000 ms vergehen
    Then ist der Kanal "2:10" als veraltet markiert
    And liegen zwischen aufeinanderfolgenden Abfragen von "2:10" jeweils mindestens 5500 ms
    And wurde "2:10" mindestens 3 Mal abgefragt

  Scenario: AK5 - Letzter Wert bleibt stehen, mit Markierung
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    When der Kanal "2:10" einen Wert geliefert hat
    And das Modul an Adresse 2 verstummt
    And die Abfrage von "2:10" ins Timeout läuft
    Then hat der Kanal "2:10" den Wert 12.0
    And ist der Kanal "2:10" als veraltet markiert

  Scenario: AK5 - Kanal hat noch nie geantwortet
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When die Abfrage von "2:10" ins Timeout läuft
    Then hat der Kanal "2:10" keinen Wert
    And ist der Kanal "2:10" als veraltet markiert

  Scenario: AK7 (Lab-Ebene) - Unveränderte Werte erzeugen keine Meldung
    Given die Module liefern "4:0=1000.0"
    And die Kanalliste "4:0"
    And ein Poll-Intervall von 1000 ms
    And ich abonniere den Änderungsstrom
    When 3 Poll-Zyklen abgelaufen sind
    Then enthält der Änderungsstrom genau 1 Meldung für "4:0"
    When die Module ab jetzt "4:0=1000.5" liefern
    And 1 Poll-Zyklus abgelaufen ist
    Then enthält der Änderungsstrom genau 2 Meldungen für "4:0"
    And enthält der Änderungsstrom eine Meldung für "4:0" mit Wert 1000.5

  Scenario: AK7 (Lab-Ebene) - Wechsel nach veraltet und zurück wird gemeldet
    Given die Module liefern "2:10=12.0"
    And die Kanalliste "2:10"
    And ein Poll-Intervall von 1000 ms
    And ich abonniere den Änderungsstrom
    When der Kanal "2:10" einen Wert geliefert hat
    And das Modul an Adresse 2 verstummt
    And die Abfrage von "2:10" ins Timeout läuft
    And das Modul an Adresse 2 wieder antwortet
    And der Kanal "2:10" wieder einen Wert geliefert hat
    Then meldete der Änderungsstrom für "2:10" nacheinander "12.0, 12.0 veraltet, 12.0"

  Scenario: AK7 (Lab-Ebene) - Snapshot für neue Abonnenten
    Given die Module liefern "1:0=0.0022, 2:10=12.0, 4:0=1000.0"
    And die Kanalliste "1:0, 2:10, 4:0"
    And ein Poll-Intervall von 1000 ms
    And das Modul an Adresse 2 antwortet nicht
    When 1 Poll-Zyklus abgelaufen ist
    And ein neuer Abonnent den Änderungsstrom abonniert
    Then enthält sein Snapshot für "1:0" den Wert 0.0022
    And enthält sein Snapshot für "4:0" den Wert 1000.0
    And enthält sein Snapshot für "2:10" keinen Wert und die Markierung veraltet
