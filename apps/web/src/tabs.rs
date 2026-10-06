//! Tab-Navigation (Spec 0006): welche Ansichten es gibt, in welcher
//! Reihenfolge, und welcher Tab beim Laden aktiv ist.
//!
//! Wie `measurements.rs` bewusst reine Logik ohne Leptos-/Browser-Bezug,
//! damit sie als Host-Test läuft. Die Komponente (Tab-Leiste, Ansichten)
//! steht in `app.rs`; sie liest das URL-Fragment NUR beim Laden und
//! schreibt es bei Klicks nicht (Entscheidung 2026-10-07) - der gewählte
//! Tab überlebt ein Neuladen also bewusst nicht.

/// Die drei Ansichten der Tab-Navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Gauge,
    Uebersicht,
    Dds,
}

impl Tab {
    /// Reihenfolge in der Tab-Leiste.
    pub const ALL: [Tab; 3] = [Tab::Gauge, Tab::Uebersicht, Tab::Dds];

    /// Beschriftung in der Tab-Leiste.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Gauge => "Gauge",
            Tab::Uebersicht => "Übersicht",
            Tab::Dds => "DDS-Steuerung",
        }
    }

    /// URL-Fragment ohne `#`, z.B. `uebersicht`.
    pub fn fragment(self) -> &'static str {
        match self {
            Tab::Gauge => "gauge",
            Tab::Uebersicht => "uebersicht",
            Tab::Dds => "dds",
        }
    }

    /// Tab aus dem URL-Fragment beim Laden (`location.hash`, mit oder ohne
    /// `#`). Unbekannt oder leer -> Start-Tab.
    pub fn from_fragment(hash: &str) -> Tab {
        let name = hash.strip_prefix('#').unwrap_or(hash);
        Tab::ALL
            .into_iter()
            .find(|tab| tab.fragment() == name)
            .unwrap_or_default()
    }
}

impl Default for Tab {
    /// Start-Tab (Spec 0006, AK1a).
    fn default() -> Self {
        Tab::Gauge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AK1a: Start-Tab ist Gauge
    #[test]
    fn start_tab_ist_gauge() {
        assert_eq!(Tab::default(), Tab::Gauge);
    }

    // AK1: drei Tabs in fester Reihenfolge mit den Beschriftungen der Spec
    #[test]
    fn reihenfolge_und_beschriftung() {
        let labels: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(labels, vec!["Gauge", "Übersicht", "DDS-Steuerung"]);
    }

    // Entscheidung 1: Fragmente #gauge, #uebersicht, #dds
    #[test]
    fn bekannte_fragmente_waehlen_den_tab() {
        assert_eq!(Tab::from_fragment("#gauge"), Tab::Gauge);
        assert_eq!(Tab::from_fragment("#uebersicht"), Tab::Uebersicht);
        assert_eq!(Tab::from_fragment("#dds"), Tab::Dds);
    }

    // `location.hash` liefert das Fragment mit '#', ohne ist es aber
    // genauso eindeutig.
    #[test]
    fn fragment_ohne_raute_wird_ebenfalls_erkannt() {
        assert_eq!(Tab::from_fragment("dds"), Tab::Dds);
    }

    // Entscheidung 1: unbekanntes oder fehlendes Fragment -> Gauge
    #[test]
    fn unbekanntes_oder_fehlendes_fragment_waehlt_gauge() {
        assert_eq!(Tab::from_fragment(""), Tab::Gauge);
        assert_eq!(Tab::from_fragment("#"), Tab::Gauge);
        assert_eq!(Tab::from_fragment("#foo"), Tab::Gauge);
        // Groß-/Kleinschreibung zählt: nur die drei festen Fragmente gelten.
        assert_eq!(Tab::from_fragment("#Uebersicht"), Tab::Gauge);
    }

    // Fragment und Tab passen in beide Richtungen zusammen (das
    // Screenshot-Werkzeug nutzt die Fragmente: shot.sh --tab NAME).
    #[test]
    fn fragment_und_tab_passen_zusammen() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_fragment(&format!("#{}", tab.fragment())), tab);
        }
        let fragments: Vec<&str> = Tab::ALL.iter().map(|t| t.fragment()).collect();
        assert_eq!(fragments, vec!["gauge", "uebersicht", "dds"]);
    }
}
