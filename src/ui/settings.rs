//! Settings view (**I12**).
//!
//! > I12: Settings view: Folders card with stackable rows and `Change...` running `dir`/`shortcuts`
//! > (output read tolerantly); Config card whose shortcut count is entries with `shortcut=on`;
//! > capped at 520 px. (D18 D58 D60)
//!
//! Replaces the groups + side panes while `state.view == View::Settings` (the gear tab toggles it,
//! so there is no back control here). Layout follows the mockup: a "Settings" header with a
//! hairline rule, then "Folders" and "Config" cards in one column of `min(width, 520)`; folder rows
//! are two columns (label | field + `Change…`) and stack below a 420 px card width. Paths are
//! `theme::mono(12.0)`. Draft text lives in egui memory (`ui.data_mut`), not in `AppState`.
//! No `ScrollArea`: `ui::draw` wraps this view in one.

use crate::app::AppState;
use crate::model::{Action, ShortcutState};
use crate::theme;
use egui::{Align, Layout, RichText};

/// Mockup `.settings { max-width: 520px }` (the column is 100% wide below that).
const MAX_W: f32 = 520.0;
/// Mockup breakpoint: rows stack (label above field) when the card is narrower.
const STACK_BELOW_CARD: f32 = 420.0;
/// Card frame overhead (group margin + stroke) added to the inner width for the breakpoint test.
const CARD_CHROME: f32 = 14.0;

/// Draw the Settings page: header, Folders card, Config card.
pub fn draw(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    let w = ui.available_width().min(MAX_W);
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(w, 0.0));
    let clip = egui::Rangef::new(rect.left(), rect.right());
    let salt = ui.auto_id_with("settings_col");
    let mut col = theme::pane_ui(ui, rect, clip, Layout::top_down(Align::Min), salt);
    let c = &mut col;
    c.spacing_mut().item_spacing.y = 0.0;

    // Header: accent title + hairline rule (mockup `.settings-head`).
    c.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::GAP;
        ui.label(theme::section_label("Settings"));
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
        ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0_f32, theme::BORDER));
    });
    c.add_space(theme::GAP);

    let enabled = state.actions_enabled();
    theme::card(c, "Folders", |ui| {
        folder_row(
            ui,
            "repo",
            "Repo folder",
            "Where ritz-get keeps every clone. Change… moves them all in one step (ritz-get dir PATH).",
            state.dir.as_ref().map(|p| p.to_string_lossy().into_owned()).as_deref(),
            enabled,
            Action::SetDir,
            actions,
        );
        ui.add_space(theme::ROW_GAP); // between the rows only: none after the last
        folder_row(
            ui,
            "shortcuts",
            "Shortcut folder",
            "Where .desktop shortcuts go. Change… moves the existing ones (ritz-get shortcuts PATH).",
            state.shortcuts_dir.as_ref().map(|p| p.to_string_lossy().into_owned()).as_deref(),
            enabled,
            Action::SetShortcutsDir,
            actions,
        );
    });

    let installed = state.listing.entries().filter(|e| e.is_installed()).count();
    let shortcuts = state.listing.entries().filter(|e| e.shortcut == ShortcutState::On).count();
    theme::card(c, "Config", |ui| {
        value_row(ui, "Config file", "~/.config/ritz-get/config");
        value_row(ui, "Programs", &format!("{installed} installed · {shortcuts} shortcuts"));
    });

    theme::card(c, "Input", |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(theme::CTRL_H);
            ui.vertical(|ui| {
                ui.label(RichText::new("Touch mode").font(theme::regular(13.0)).color(theme::TEXT));
                ui.add(egui::Label::new(RichText::new("Drag with the mouse to scroll lists (for touchscreens).").font(theme::regular(12.0)).color(theme::TEXT_DIM)).wrap());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if super::side::auto_switch(ui, state.touch_mode, true).clicked() {
                    actions.push(Action::SetTouchMode(!state.touch_mode));
                }
            });
        });
    });

    let h = (col.min_rect().bottom() - rect.top()).max(0.0);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(rect.min, egui::vec2(w, h)));
}

/// Label left, dim mono value right (mockup `.row` + `.row-val`).
fn value_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(theme::CTRL_H);
        ui.label(RichText::new(label).font(theme::regular(13.0)).color(theme::TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).font(theme::mono(12.0)).color(theme::TEXT_DIM)).wrap());
        });
    });
}

/// One folder row: label + editable mono path field + trailing `Change…` (right-to-left).
/// Two columns at card width >= 420, stacked (label above field) below.
#[allow(clippy::too_many_arguments)]
fn folder_row(
    ui: &mut egui::Ui,
    key: &str,
    label: &str,
    tip: &str,
    current: Option<&str>,
    enabled: bool,
    make: impl Fn(String) -> Action,
    actions: &mut Vec<Action>,
) {
    let id = ui.id().with(("settings_draft", key));
    let cur = current.unwrap_or("").to_string();
    // (buffer, state value the buffer was initialised from): re-init when the state's value changes.
    let (mut buf, seen): (String, String) =
        ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| (cur.clone(), cur.clone()));
    if seen != cur {
        buf = cur.clone();
    }
    let w = ui.available_width();
    let stacked = w + CARD_CHROME < STACK_BELOW_CARD;
    let mut apply = false;

    let field = |ui: &mut egui::Ui, buf: &mut String, apply: &mut bool| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let btn = ui.add_enabled(enabled, theme::secondary_button("Change…"));
            if btn.clicked() {
                *apply = true;
            }
            let hint = if current.is_none() { "unknown" } else { "" };
            // Unfocused and too long: show the path with its LEFT elided (the end stays visible), full
            // path in the tooltip. Focused: the real buffer, editable. Edits made on a shown copy are dropped.
            let edit_id = ui.id().with(("settings_edit", key));
            let focused = ui.memory(|m| m.has_focus(edit_id));
            let avail = ui.available_width() - 16.0 - 6.0;
            let fits = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), theme::mono(12.0), theme::TEXT).size().x <= avail;
            let mut shown = if focused { buf.clone() } else { elide_left(buf, fits) };
            let elided = !focused && shown != *buf;
            let r = ui.add(
                egui::TextEdit::singleline(if focused { &mut *buf } else { &mut shown })
                    .id(edit_id)
                    .font(theme::mono(12.0))
                    .hint_text(theme::placeholder(hint))
                    .desired_width(ui.available_width())
                    .margin(egui::vec2(8.0, 3.0))
                    .min_size(egui::vec2(0.0, theme::CTRL_H)), // mockup `.input { height: var(--ctl) }`
            );
            let r = if elided { r.on_hover_text(buf.as_str()) } else { r };
            if enabled && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                *apply = true;
            }
        });
    };
    let lbl = |ui: &mut egui::Ui| {
        ui.label(RichText::new(label).font(theme::regular(13.0)).color(theme::TEXT)).on_hover_text(tip);
    };

    if stacked {
        lbl(ui);
        ui.add_space(2.0);
        field(ui, &mut buf, &mut apply);
    } else {
        // Deviation from the mockup's 1fr|1fr grid: a fixed label column so the path field gets the room.
        let label_w = 120.0_f32.min(w * 0.35);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::GAP;
            ui.allocate_ui_with_layout(egui::vec2(label_w, theme::CTRL_H), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_size(egui::vec2(label_w, theme::CTRL_H));
                lbl(ui)
            });
            let fw = (w - label_w - theme::GAP).max(0.0);
            ui.allocate_ui_with_layout(egui::vec2(fw, theme::CTRL_H), Layout::top_down(Align::Min), |ui| {
                ui.set_min_width(fw);
                field(ui, &mut buf, &mut apply)
            });
        });
    }
    if apply {
        let p = buf.trim().to_string();
        if !p.is_empty() {
            actions.push(make(p));
        }
    }
    ui.data_mut(|d| d.insert_temp(id, (buf, cur)));
}

/// `text` if `fits`, else "\u{2026}" + the longest tail that fits, started at a `/` when it has one
/// ("\u{2026}/.local/share/applications"), so the end of a path stays readable.
pub(crate) fn elide_left(text: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(text) {
        return text.to_owned();
    }
    let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    // shortest cut (longest tail) that fits; fits() is monotone in the tail length
    let mut cut = *starts.last().unwrap_or(&0);
    for &i in &starts {
        if fits(&format!("\u{2026}{}", &text[i..])) {
            cut = i;
            break;
        }
    }
    let tail = &text[cut..];
    let tail = match tail.find('/') {
        Some(0) | None => tail,
        Some(k) => &tail[k..],
    };
    format!("\u{2026}{tail}")
}

#[cfg(test)]
mod elide_tests {
    use super::elide_left;

    #[test]
    fn keeps_the_end_of_the_path() {
        let p = "/home/user/.local/share/applications";
        assert_eq!(elide_left(p, |t| t.chars().count() <= 100), p);
        // limit 24: the cut lands inside ".local", snapped forward to the next '/'
        assert_eq!(elide_left(p, |t| t.chars().count() <= 24), "\u{2026}/share/applications");
        assert_eq!(elide_left(p, |t| t.chars().count() <= 27), "\u{2026}/.local/share/applications");
        // no slash in the tail: plain char cut
        assert_eq!(elide_left("abcdefghij", |t| t.chars().count() <= 4), "\u{2026}hij");
        // nothing fits: still the last char
        assert_eq!(elide_left("abc", |t| t.is_empty()), "\u{2026}c");
    }
}
