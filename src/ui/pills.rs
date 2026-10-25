//! The one pill mapping (D19 D21 D24 D53 D54), used by the cards (`groups`) and the side hero
//! (`side`). [`pills`] is a pure function of `(state, entry)`; [`show`] paints one.
//!
//! | condition                                   | pill                                  |
//! |---------------------------------------------|---------------------------------------|
//! | website                                     | WEB (ACCENT)                          |
//! | installed/unlisted, `status=fetch-failed`   | FETCH FAILED (DANGER)                 |
//! | installed/unlisted, has update              | UPDATE AVAILABLE (WARN)               |
//! | installed/unlisted, `status=current`        | UP TO DATE (GOOD)                     |
//! | installed, `status=unknown`/modified-only   | none (D24: the installed version only)|
//! | `state=offline` (not installed)             | OFFLINE (TEXT_DIM)                    |
//! | `state=available`                           | NOT INSTALLED (TEXT_DIM)              |
//! | session `Badge::Needs(tools)`               | NEEDS <TOOL> per tool (DANGER); for a program that is not installed it replaces NOT INSTALLED / OFFLINE |
//! | session `Badge::SetupNeeded`                | SETUP NEEDED (WARN)                   |
//! | `held`                                      | pause glyph (TEXT_DIM), tooltip "Auto-update off (held)" |
//! | `status=modified`                           | MODIFIED (WARN, D53)                  |
//! | `setup` steps                               | SETUP (ACCENT)                        |
//! | `state.last_failed(e)`                      | FAILED (DANGER)                       |

use egui::Color32;

use crate::app::AppState;
use crate::model::{Badge, Entry, State, Status};
use crate::theme::{self, PillKind};

/// One status pill: a label (drawn uppercase) or a lone glyph, a colour and an optional tooltip.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub text: String,
    pub color: Color32,
    /// A glyph-only pill (the "held" pause pill).
    pub glyph: bool,
    pub tip: Option<String>,
}

impl Pill {
    fn kind(k: PillKind) -> Pill {
        Pill { text: k.label().to_owned(), color: k.color(), glyph: false, tip: None }
    }

    fn tip(mut self, tip: impl Into<String>) -> Pill {
        self.tip = Some(tip.into());
        self
    }
}

/// The pills of `e`, in display order (see the module table).
pub fn pills(state: &AppState, e: &Entry) -> Vec<Pill> {
    let mut v = Vec::new();
    let needs: Option<&Vec<String>> = match state.badge(e) {
        Some(Badge::Needs(tools)) => Some(tools),
        _ => None,
    };
    let needs_pills = |v: &mut Vec<Pill>, tools: &[String]| {
        if tools.is_empty() {
            v.push(Pill::kind(PillKind::Needs).tip("A tool this program needs is missing"));
        }
        for tool in tools {
            v.push(
                Pill { text: format!("Needs {tool}"), color: PillKind::Needs.color(), glyph: false, tip: None }
                    .tip("A tool this program needs is missing"),
            );
        }
    };

    if e.is_website() {
        v.push(Pill::kind(PillKind::Web));
    } else if e.is_installed() {
        if e.status == Status::FetchFailed {
            v.push(Pill::kind(PillKind::FetchFailed).tip("Could not check for updates"));
        } else if e.has_update() {
            v.push(Pill::kind(PillKind::Update));
        } else if e.status == Status::Current {
            v.push(Pill::kind(PillKind::Ok));
        }
        // Unknown / modified-only / none: D24, the version line says it all.
        if let Some(tools) = needs {
            needs_pills(&mut v, tools);
        }
    } else if let Some(tools) = needs {
        needs_pills(&mut v, tools);
    } else if e.state == State::Offline {
        v.push(Pill::kind(PillKind::Offline).tip("You're offline"));
    } else {
        v.push(Pill::kind(PillKind::Missing));
    }

    if matches!(state.badge(e), Some(Badge::SetupNeeded)) {
        v.push(Pill::kind(PillKind::SetupNeeded).tip("Needs root setup: run `ritz-get install` in a terminal"));
    }
    if e.held {
        v.push(Pill { text: theme::glyph::PAUSE.to_owned(), color: PillKind::Held.color(), glyph: true, tip: None }.tip("Auto-update off (held)"));
    }
    if e.is_modified() {
        let n = e.dirty.unwrap_or(0);
        let tip = if n > 0 {
            format!("{n} changed file{}; updates are skipped until you reset", if n == 1 { "" } else { "s" })
        } else {
            "Local changes; updates are skipped until you reset".to_owned()
        };
        v.push(Pill::kind(PillKind::Modified).tip(tip));
    }
    if !e.setup.is_empty() {
        v.push(Pill::kind(PillKind::Setup).tip("Has manual setup steps"));
    }
    if state.last_failed(e) {
        v.push(
            Pill { text: "Failed".to_owned(), color: theme::DANGER, glyph: false, tip: None }
                .tip("The last update of this program failed (see the log)"),
        );
    }
    v
}

/// Paint one pill (with its tooltip) at the cursor.
pub fn show(ui: &mut egui::Ui, p: &Pill) -> egui::Response {
    let r = if p.glyph { theme::pill_glyph(ui, &p.text, p.color) } else { theme::pill(ui, &p.text, p.color) };
    match &p.tip {
        Some(t) => r.on_hover_text(t),
        None => r,
    }
}

/// Width [`show`] will allocate for `p` (same font, letter spacing and padding as the theme's pill).
pub fn width(ui: &egui::Ui, p: &Pill) -> f32 {
    let (font, pad) = if p.glyph { (egui::FontId::proportional(10.0), 6.0) } else { (theme::medium(12.0), 7.0) };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &p.text.to_uppercase(),
        0.0,
        egui::text::TextFormat { font_id: font.clone(), extra_letter_spacing: font.size * 0.03, ..Default::default() },
    );
    ui.painter().layout_job(job).size().x + pad * 2.0 + 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Badge, LastRun};

    fn st() -> AppState {
        AppState::sample()
    }

    fn labels(s: &AppState, id: &str) -> Vec<String> {
        let e = s.listing.find(id).unwrap_or_else(|| panic!("sample has no {id}"));
        pills(s, e).into_iter().map(|p| p.text).collect()
    }

    /// Every sample entry: the mapping in the module table, in order.
    #[test]
    fn mapping_across_the_sample_entries() {
        let s = st();
        for e in s.listing.entries() {
            let got: Vec<String> = pills(&s, e).into_iter().map(|p| p.text).collect();
            // first pill = status
            if e.is_website() {
                assert_eq!(got[0], "Website", "{}", e.id());
            } else if e.is_installed() {
                match (&e.status, e.has_update()) {
                    (&Status::FetchFailed, _) => assert_eq!(got[0], "Fetch failed", "{}", e.id()),
                    (_, true) => assert_eq!(got[0], "Update available", "{}", e.id()),
                    (&Status::Current, false) => assert_eq!(got[0], "Up to date", "{}", e.id()),
                    // D24: unknown / modified-only: no status pill at all
                    _ => assert!(!["Offline", "Not installed", "Up to date", "Update available"].contains(&got.first().map_or("", String::as_str)), "{}: {got:?}", e.id()),
                }
            } else {
                assert_eq!(got[0], if e.state == State::Offline { "Offline" } else { "Not installed" }, "{}", e.id());
            }
            // never OFFLINE for an installed entry (the side hero used to show it for `unknown`)
            if e.is_installed() {
                assert!(!got.contains(&"Offline".to_owned()), "{}: {got:?}", e.id());
            }
            assert_eq!(got.contains(&"Modified".to_owned()), e.is_modified(), "{}", e.id());
            assert_eq!(got.contains(&"Setup".to_owned()), !e.setup.is_empty(), "{}", e.id());
            assert_eq!(got.contains(&theme::glyph::PAUSE.to_owned()), e.held, "{}", e.id());
        }
        assert_eq!(labels(&s, "Helpers/ts6tray"), ["Update available", "Setup"]);
        assert_eq!(labels(&s, "Software/ForzaTelemetryV3Launcher"), ["Not installed"]);
        assert_eq!(labels(&s, "Websites/cs2-alias-cycling"), ["Website"]);
        let geo = labels(&s, "Software/geofinder");
        assert!(geo.contains(&"Modified".to_owned()));
        assert!(labels(&s, "Software/yt-dlp-gui").contains(&theme::glyph::PAUSE.to_owned()));
    }

    #[test]
    fn unknown_status_shows_no_pill_and_offline_only_when_not_installed() {
        let mut s = st();
        let id = "Helpers/ts6tray";
        let e = s.listing.sections.iter_mut().flat_map(|x| x.entries.iter_mut()).find(|e| e.id() == id).unwrap();
        e.status = Status::Unknown;
        e.behind = None;
        e.setup.clear();
        assert!(labels(&s, id).is_empty(), "installed + unknown: the version line only (D24)");
        let e = s.listing.sections.iter_mut().flat_map(|x| x.entries.iter_mut()).find(|e| e.name == "ForzaTelemetryV3Launcher").unwrap();
        e.state = State::Offline;
        assert_eq!(labels(&s, "Software/ForzaTelemetryV3Launcher"), ["Offline"]);
    }

    #[test]
    fn needs_replaces_not_installed_and_follows_status_when_installed() {
        let mut s = st();
        let two = Badge::Needs(vec!["cmake".into(), "pkg-config".into()]);
        s.badges.insert("ForzaTelemetryV3Launcher".into(), two.clone());
        assert_eq!(labels(&s, "Software/ForzaTelemetryV3Launcher"), ["Needs cmake", "Needs pkg-config"]);
        s.badges.insert("ts6tray".into(), two);
        assert_eq!(labels(&s, "Helpers/ts6tray"), ["Update available", "Needs cmake", "Needs pkg-config", "Setup"]);
        s.badges.insert("ts6tray".into(), Badge::SetupNeeded);
        assert_eq!(labels(&s, "Helpers/ts6tray"), ["Update available", "Setup needed", "Setup"]);
    }

    #[test]
    fn failed_pill_comes_last_and_held_modified_order() {
        let mut s = st();
        s.last_run = Some(LastRun {
            time: "2026-10-24T03:00:01".into(),
            updated: 0,
            skipped: 0,
            failed: 2,
            failed_names: vec!["geofinder".into(), "yt-dlp-gui".into()],
        });
        let geo = labels(&s, "Software/geofinder");
        assert_eq!(geo.last().map(String::as_str), Some("Failed"));
        let yt = labels(&s, "Software/yt-dlp-gui");
        let pause = yt.iter().position(|t| *t == theme::glyph::PAUSE).expect("held");
        assert!(pause < yt.len() - 1 && yt.last().map(String::as_str) == Some("Failed"));
        assert!(labels(&s, "Helpers/ts6tray").iter().all(|t| t != "Failed"));
    }
}
