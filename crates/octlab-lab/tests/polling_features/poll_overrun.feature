Feature: Poll-Zyklus länger als das Intervall (Spec 0005, AK10)

  Zyklen überlappen nicht, verpasste Takte werden nicht nachgeholt, der
  nächste Zyklus startet sofort. Pro Überlauf-Episode gibt es genau eine
  Warnung (gezählt in der Poll-Statistik), die gemessene Zykluszeit ist
  abrufbar.

  Background:
    Given die Module liefern "1:0=0.0022, 2:10=12.0, 4:0=1000.0"
    And die Kanalliste "1:0, 2:10, 4:0"
    And ein Poll-Intervall von 1000 ms
    And jedes Modul antwortet erst nach 400 ms

  Scenario: AK10 - Kein Überlappen, kein Nachholen
    When 6000 ms vergehen
    Then wurde "1:0" bei "0, 1200, 2400, 3600, 4800" ms abgefragt
    And lagen zwischen zwei Abfragen jeweils mindestens 400 ms

  Scenario: AK10 - Eine Warnung pro Überlauf-Episode
    When 5000 ms vergehen
    Then zählt die Poll-Statistik 1 Überlauf-Episode
    When jedes Modul ab jetzt sofort antwortet
    And 3000 ms vergehen
    Then zählt die Poll-Statistik 1 Überlauf-Episode
    When jedes Modul ab jetzt erst nach 400 ms antwortet
    And 3000 ms vergehen
    Then zählt die Poll-Statistik 2 Überlauf-Episoden

  Scenario: AK10 - Gemessene Zykluszeit ist abrufbar
    When 3000 ms vergehen
    Then meldet die Poll-Statistik eine letzte Zykluszeit von 1200 ms und ein Intervall von 1000 ms

  # Unterscheidet "nicht nachholen" von einem festen Takt-Raster, das
  # verpasste Takte nach einem Überlauf sofort nachholt: Zyklen beginnen
  # bei 0, 1200, 2400 ms; der dritte endet bei 3200 ms (nur noch 800 ms,
  # da die Module ab 3000 ms sofort antworten). Ohne Nachholen startet der
  # nächste Zyklus regulär ein Intervall nach dem letzten Beginn (3400 ms),
  # ein nachholendes Raster dagegen sofort (3200 ms) und danach bei 4000,
  # 5000 ms.
  Scenario: AK10 - Nach dem Überlauf werden verpasste Takte nicht nachgeholt
    When 3000 ms vergehen
    And jedes Modul ab jetzt sofort antwortet
    And 3000 ms vergehen
    Then wurde "1:0" bei "400, 1400, 2400" ms abgefragt
