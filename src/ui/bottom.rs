//! Status bar (**I11**). The log drawer that used to sit above it is gone: the build log lives in the
//! progress dialog (its "Show log" expander) and in the log dialog (the "Log" link here, see
//! `ui::logview` and `ui::modals`).
//!
//! **Status bar** (mockup `.status`): left = dot (GOOD; WARN while checking or offline) + "checked
//! 2 min ago" (or "checking…") + "offline" + "N updates" + "N installed" + "N held" + "last update
//! failed (see log)" (DANGER, click opens the log dialog); while a run is hidden ("Run in
//! background") its "Installing NAME…" / "Updating 3 of 5…" (ACCENT, click shows the progress dialog
//! again) comes first; right = "Log" (opens the log dialog) + "Report a problem with ritz-get" link
//! (`Action::OpenUrl`) + "Ritze03". The bar is a single row at every width: when it does not fit,
//! items are dropped in this order: "Ritze03", "N held", "N installed", "N updates", then the
//! link is shortened to "Report a problem". "checked" / "offline" / "last update failed" are never
//! dropped. A repaint is requested every 30 s so the "ago" text ticks.

use std::sync::Arc;
use std::time::Duration;

use egui::{pos2, vec2, Color32, CursorIcon, Galley, Rect, Sense, Stroke};

use crate::app::AppState;
use crate::model::{Action, Modal};
use crate::theme;

const REPORT_URL: &str = "https://github.com/Ritze03/ritz-get-gui/issues/new";

// ---------------------------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------------------------

/// "just now", "2 min ago", "3 h ago", "2 d ago" (mockup: "checked 2 min ago").
pub fn ago(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Link {
    /// "last update failed (see log)": opens the log dialog.
    Failed,
    /// "Report a problem ...".
    Report,
    /// "Log": opens the log dialog.
    Log,
    /// "Installing NAME…": shows the hidden progress dialog.
    Run,
}

impl Link {
    fn action(self) -> Action {
        match self {
            Link::Report => Action::OpenUrl(REPORT_URL.to_owned()),
            Link::Failed | Link::Log => Action::ShowModal(Modal::Log),
            Link::Run => Action::ShowModal(Modal::Run),
        }
    }
}

struct Item {
    galley: Arc<Galley>,
    color: Color32,
    link: Option<Link>,
}

/// DANGER when ritz-get is missing or refuses to talk to us, WARN while checking, offline
/// or never checked, GOOD only when connected and checked.
fn status_dot(missing: bool, refused: bool, loading: bool, offline: bool, checked: bool) -> Color32 {
    if missing || refused {
        theme::DANGER
    } else if loading || offline || !checked {
        theme::WARN
    } else {
        theme::GOOD
    }
}

/// One row of items at every width. Layout is computed first (so the drop order is exact), then
/// painted left from the dot and right-to-left from the edge.
pub fn draw_status(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    const DOT: f32 = 7.0;
    const GAP: f32 = 8.0;
    let font = theme::regular(11.0);
    let item = |text: &str, color: Color32, link: Option<Link>| Item {
        galley: ui.painter().layout_no_wrap(text.to_owned(), font.clone(), color),
        color,
        link,
    };

    if state.checked_at.is_some() {
        // tick "checked N min ago"
        ui.ctx().request_repaint_after(Duration::from_secs(30));
    }
    let counts = state.counts();
    let held = state.listing.entries().filter(|e| e.held).count();

    let offline = state.listing.is_offline();
    let broken = state.ritz_get_missing || state.refused.is_some();
    let dot = status_dot(state.ritz_get_missing, state.refused.is_some(), state.loading, offline, state.checked_at.is_some());
    let checked = if state.loading {
        "checking…".to_owned()
    } else if let Some(at) = state.checked_at {
        format!("checked {}", ago(at.elapsed()))
    } else {
        "not checked yet".to_owned()
    };
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });

    // (text, colour, link, drop level: 0 = never dropped, k = dropped from level k on)
    let mut left: Vec<(String, Color32, Option<Link>, u8)> = Vec::new();
    // No usable ritz-get: the counts would be fake zeros, so say what is wrong instead.
    if broken {
        let msg = if state.ritz_get_missing { "ritz-get not found" } else { "ritz-get-gui needs an update" };
        left.push((msg.into(), theme::DANGER, None, 0));
    } else {
    // offline before the first successful check: "offline" says it all, not "not checked yet · offline"
    if !(offline && state.checked_at.is_none() && !state.loading) {
        left.push((checked, theme::TEXT_DIM, None, 0));
    }
    if offline {
        left.push(("offline".into(), theme::WARN, None, 0));
    }
    left.push((plural(counts.updates, "update", "updates"), theme::TEXT_DIM, None, 4));
    left.push((format!("{} installed", counts.installed), theme::TEXT_DIM, None, 3));
    if held > 0 {
        left.push((format!("{held} held"), theme::TEXT_DIM, None, 2));
    }
    }
    if let Some(label) = state.reopen_label() {
        // first, so it is the last to be cut when the bar is narrow
        left.insert(0, (label, theme::ACCENT, Some(Link::Run), 0));
    }
    if state.last_update_failed() {
        left.push(("last update failed (see log)".into(), theme::DANGER, Some(Link::Failed), 0));
    }

    let avail = ui.available_width();
    // Level L keeps the items whose drop level is 0 or > L. Level 0 = everything; 1 drops
    // "Ritze03"; 2 "N held"; 3 "N installed"; 4 "N updates"; 5 shortens the link.
    let build = |level: u8| {
        let mut items: Vec<Item> = Vec::new();
        let sep = ui.painter().layout_no_wrap("·".into(), font.clone(), theme::TEXT_FAINT);
        for (text, color, link, drop) in &left {
            if *drop != 0 && *drop <= level {
                continue;
            }
            if !items.is_empty() {
                items.push(Item { galley: sep.clone(), color: theme::TEXT_FAINT, link: None });
            }
            items.push(item(text, *color, *link));
        }
        let link_text = if level >= 5 { "Report a problem" } else { "Report a problem with ritz-get" };
        // left to right: the link, then "Ritze03" at the window edge
        let mut right = vec![item("Log", theme::ACCENT, Some(Link::Log)), item(link_text, theme::ACCENT, Some(Link::Report))];
        if level == 0 {
            right.push(item("Ritze03", theme::TEXT_DIM, None));
        }
        (items, right)
    };
    let width = |left: &[Item], right: &[Item]| {
        let sum = |v: &[Item]| v.iter().map(|i| i.galley.size().x).sum::<f32>() + GAP * v.len().saturating_sub(1) as f32;
        DOT + GAP + sum(left) + GAP * 2.0 + sum(right)
    };
    let mut level = 0;
    let (mut lefts, mut rights) = build(level);
    while level < 5 && width(&lefts, &rights) > avail {
        level += 1;
        (lefts, rights) = build(level);
    }

    let h = 16.0;
    let (bar, _) = ui.allocate_exact_size(vec2(avail, h), Sense::hover());
    let cy = bar.center().y;
    let paint = |ui: &egui::Ui, it: &Item, x: f32, color: Color32, hover: bool| {
        let pos = pos2(x, cy - it.galley.size().y / 2.0);
        ui.painter().galley(pos, it.galley.clone(), color);
        if hover {
            let y = pos.y + it.galley.size().y - 1.0;
            ui.painter().hline(pos.x..=pos.x + it.galley.size().x, y, Stroke::new(1.0_f32, color));
        }
    };

    // right group, laid out from the right edge
    let mut x = bar.right();
    for it in rights.iter().rev() {
        x -= it.galley.size().x;
        let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(it.galley.size().x, h));
        let color = it.color;
        let mut hover = false;
        if let Some(link) = it.link {
            let r = ui.interact(rect, ui.id().with(("status_link", link as u8)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            hover = r.hovered();
            if r.clicked() {
                actions.push(link.action());
            }
        }
        paint(ui, it, x, color, hover);
        x -= GAP;
    }
    let right_edge = x + GAP;

    // left group
    let dot_rect = Rect::from_center_size(pos2(bar.left() + DOT / 2.0, cy), vec2(DOT, DOT));
    ui.painter().circle_filled(dot_rect.center(), DOT / 2.0, dot);
    let mut x = bar.left() + DOT + GAP;
    for it in &lefts {
        let w = it.galley.size().x;
        if x + w > right_edge - GAP && it.link.is_none() && x > bar.left() + DOT + GAP {
            break;
        }
        let color = it.color;
        let mut hover = false;
        if let Some(link) = it.link {
            let rect = Rect::from_min_size(pos2(x, bar.top()), vec2(w, h));
            let r = ui.interact(rect, ui.id().with(("status_link", link as u8)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            hover = r.hovered();
            if r.clicked() {
                actions.push(link.action());
            }
        }
        paint(ui, it, x, color, hover);
        x += w + GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LastRun, LogKind};
    use crate::ui::test_render::{assert_inside_panes, render_app, SIZES};

    #[test]
    fn dot_colours_follow_seed() {
        use theme::{DANGER, GOOD, WARN};
        assert_eq!(status_dot(true, false, false, false, true), DANGER);
        assert_eq!(status_dot(false, true, true, false, true), DANGER);
        assert_eq!(status_dot(false, false, true, false, true), WARN);
        assert_eq!(status_dot(false, false, false, true, true), WARN);
        assert_eq!(status_dot(false, false, false, false, false), WARN);
        assert_eq!(status_dot(false, false, false, false, true), GOOD);
    }

    #[test]
    fn ago_wording() {
        assert_eq!(ago(Duration::from_secs(5)), "just now");
        assert_eq!(ago(Duration::from_secs(120)), "2 min ago");
        assert_eq!(ago(Duration::from_secs(7300)), "2 h ago");
        assert_eq!(ago(Duration::from_secs(3 * 86_400)), "3 d ago");
    }

    /// Worst case for the status bar: offline, a failed last run, a hidden run with a long name,
    /// held entries: nothing paints outside the status pane at 1280, 900 and 600 px, and a very
    /// long log does not matter any more (it is not drawn here).
    #[test]
    fn status_bar_stays_inside_its_pane() {
        for hidden in [false, true] {
            for (w, h) in SIZES {
                let mut state = AppState::sample();
                state.checked_at = Some(std::time::Instant::now());
                state.last_run = Some(LastRun { time: "2026-10-22T10:00:00".into(), updated: 1, skipped: 0, failed: 2, failed_names: vec!["a".into(), "b".into()] });
                for i in 0..5000 {
                    state.push_log(if i % 7 == 0 { LogKind::Err } else { LogKind::Info }, format!("line {i}: {}", "x".repeat(i % 300)));
                }
                if hidden {
                    let cmd = crate::model::Command::Install { name: "ts6tray".into(), branch: None };
                    state.push_log(LogKind::Cmd, "$ ritz-get install ts6tray");
                    state.begin_run(&cmd);
                    state.modal = None;
                }
                let name = format!("status_{}_{}", if hidden { "hidden" } else { "plain" }, w as u32);
                let (ctx, out, tex, rects) = render_app(&state, w, h);
                crate::ui::test_render::snapshot(&ctx, &out, &tex, w as u32, h as u32, &name);
                assert_inside_panes(&out, &rects, w, h, &name);
                assert!(rects.status.height() < 40.0, "{name}: status bar height {}", rects.status.height());
            }
        }
    }
}
