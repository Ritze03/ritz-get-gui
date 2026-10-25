//! Modals + the "ritz-get not found" banner + the porcelain-refused dialog (**I13**).
//!
//! Look = mockup `.modal-back` / `.modal`: HEAD@72 % backdrop that swallows clicks, a centred
//! window (PANEL, 1 px BORDER, radius 7, soft shadow), bold 13 px title, 12 px TEXT_DIM body at 1.5
//! line height, buttons right-aligned. Built on `egui::Modal` (Order::Foreground area + a full-screen
//! backdrop; clicks on the backdrop and Esc close).
//!
//! * [`Modal::Reset`] (D22 D32 D53 D61): 420 px, `[Open folder] [Cancel] [Discard and reset]`; Cancel gets
//!   the focus, so Enter cancels. The text promises nothing beyond "the old version stays if the
//!   rebuild fails".
//! * [`Modal::DeleteData`]: 420 px, "Delete NAME\u{2019}s data?", the data folders (mono, with their sizes, in a
//!   FIELD box that scrolls when there are many), "This can\u{2019}t be undone.", `[Cancel] [Delete]`; Cancel gets
//!   the focus and Delete is the danger button, so nothing is ever deleted by one click or one Enter.
//!   Delete is off while the program is still installed (ritz-get refuses then).
//! * [`Modal::Run`] (design canvas boards 2a, 3, 4): the progress dialog of an install / update /
//!   repair / switch / reset / remove (460 px) and of Update all (500 px), 20 px padding, 16 px gap,
//!   28 px buttons, 56 px (48 px) app icon with a status badge. Its content is `state.run`:
//!   - running: indeterminate sweep bar, the newest log line (mono), "Show log" expander,
//!     `[Run in background]`. Esc / backdrop do the same; the cards keep their bars and the status bar
//!     (Update all: the top bar button) offers the way back.
//!   - done: ✓ + what changed + `[Done]`; after a fresh install whose refreshed listing says column 17
//!     is `off` (D47) it is the 2a card: "Add it to the start menu?" with a menu-entry preview,
//!     `[Not now] [Add to start menu]` (Add has the focus).
//!   - failed: ✕, a FAILED pill with the reason, `[Show log] [Close]`.
//!   - exit 3 (D32 D54 D58): "Missing tools", the merged `deps:` command in a selectable mono box with
//!     `Copy command` ("Copied" for 1.5 s) and "then press Install again"; without a `deps:` line
//!     (unknown distro) the raw `missing:` lines and a plain `Copy`.
//!   - Update all: a count bar and one row per program (icon, name, mono line, status pill, own thin bar).
//! * [`Modal::Log`]: the whole log (mono, scrolls, Copy log); the status bar's "Log" link opens it.
//! * `state.refused` (D32): "Your ritz-get is newer than this version ... (porcelain N)." with an
//!   `Update ritz-get-gui` button. Not dismissible (the app has nothing to show until it is fixed);
//!   the button deliberately ignores `refused` in its enabled check, see the report on `dispatch`.
//! * [`draw_banner`] (D58): "ritz-get not found" strip with `Retry`.

use egui::{Align, Color32, Frame, Id, Layout, Margin, RichText, Sense, Stroke};

use crate::app::{AppState, CardProgress, Progress, Run, RunPhase, Verb};
use crate::icons;
use crate::model::{Action, Entry, LogKind, Modal};
use crate::runner::Outcome;
use crate::theme::{self, glyph};

/// Mockup `.modal { max-width: 420px }` and `.modal-back { padding: 16px }`.
const MODAL_W: f32 = 420.0;
const SCREEN_PAD: f32 = 16.0;
const MODAL_PAD: i8 = 12;
/// The progress dialogs (canvas): 460 px / 500 px (Update all), 20 px padding, 16 px between blocks.
const RUN_W: f32 = 460.0;
const ALL_W: f32 = 500.0;
const RUN_PAD: i8 = 20;
const RUN_GAP: f32 = 16.0;
/// The log dialog.
const LOG_W: f32 = 680.0;
/// Canvas buttons are 28 px high.
const BTN_H: f32 = 28.0;
/// Canvas `--track`: the empty part of a bar.
const TRACK: Color32 = Color32::from_rgb(0x24, 0x28, 0x30);
/// Canvas `--danger-t`: red text on a dark tint.
const DANGER_T: Color32 = Color32::from_rgb(0xF2, 0x7B, 0x7A);
/// Canvas `.bar.dim`: the finished bar of an "up to date" row.
const BAR_DIM: Color32 = Color32::from_rgb(0x5F, 0x69, 0x7A);
/// Font Awesome `chevron-up` (the expander when the log is open).
const CHEVRON_UP: &str = "\u{F077}";
/// Mockup `--shade`: HEAD at 72 % (backdrop and shadow).
fn shade() -> Color32 {
    theme::with_alpha(theme::HEAD, 0.72)
}

/// Show the open modal / refused dialog, if any.
pub fn draw(ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
    // Porcelain refused outranks everything: the listing is empty and nothing else works.
    if let Some(n) = state.refused {
        refused(ctx, state, n, actions);
        return;
    }
    match &state.modal {
        Some(Modal::Reset { id }) => reset(ctx, state, id, actions),
        Some(Modal::DeleteData { id }) => delete_data(ctx, state, id, actions),
        Some(Modal::Run) => run(ctx, state, actions),
        Some(Modal::Log) => log_modal(ctx, state, actions),
        None => {
            ctx.data_mut(|d| d.remove_temp::<(u64, u8)>(Id::new("modal_focus_frames")));
        }
    }
}

// ---- Reset -------------------------------------------------------------------------------------

fn reset(ctx: &egui::Context, state: &AppState, id: &str, actions: &mut Vec<Action>) {
    let entry = state.listing.find(id);
    let name = entry.map_or_else(|| id.rsplit('/').next().unwrap_or(id).to_owned(), |e| e.name.clone());
    let display = entry.map_or(name.as_str(), Entry::display).to_owned();
    let changed = match entry.and_then(|e| e.dirty) {
        Some(1) => "1 changed file".to_owned(),
        Some(n) => format!("{n} changed files"),
        None => "Your local changes".to_owned(),
    };
    let folder = state.dir.as_deref().map(|d| d.join(&name).display().to_string());

    let close = show(ctx, "modal_reset", true, |ui| {
        title(ui, &format!("Discard local changes to {display}?"));
        body(ui, &format!("{changed} will be lost."));
        if let Some(f) = &folder {
            let font = theme::mono(11.5);
            let shown = theme::wrap_at_slashes(ui, &tilde(f), &font, ui.available_width());
            ui.add(egui::Label::new(RichText::new(shown).font(font).color(theme::TEXT_DIM)).wrap());
        }
        body(
            ui,
            "ritz-get downloads a fresh copy and rebuilds it. If the rebuild fails, the old version stays. \
             Want to keep your edits? Open the folder and copy them out first.",
        );
        let mut close = false;
        buttons(ui, |ui| {
            // right-to-left: the last pushed is leftmost
            let go = ui.add_enabled(state.actions_enabled(), theme::danger_button("Discard and reset"));
            theme::hover_outline(ui, &go, theme::DANGER);
            if go.clicked() {
                // Close first: the reset opens the progress dialog, which the close must not take away.
                actions.push(Action::CloseModal);
                actions.push(Action::Reset { name: name.clone() });
            }
            let cancel = ui.add(theme::secondary_button("Cancel"));
            focus_first_frames(ui.ctx(), &cancel, "reset");
            if cancel.clicked() {
                close = true;
            }
            let open = ui.add_enabled(folder.is_some(), theme::secondary_button("Open folder"));
            if let (true, Some(f)) = (open.clicked(), &folder) {
                actions.push(Action::OpenFolder(f.clone()));
            }
        });
        close
    });
    if close {
        actions.push(Action::CloseModal);
    }
}

// ---- Delete data -------------------------------------------------------------------------------

fn delete_data(ctx: &egui::Context, state: &AppState, id: &str, actions: &mut Vec<Action>) {
    let entry = state.listing.find(id);
    let name = entry.map_or_else(|| id.rsplit('/').next().unwrap_or(id).to_owned(), |e| e.name.clone());
    let display = entry.map_or(name.as_str(), Entry::display).to_owned();
    let data = entry.map_or(&[][..], |e| e.data.as_slice());
    let installed = entry.is_some_and(Entry::is_installed);
    // Room for the list: the rest of the dialog is about 190 px.
    let list_h = room(ctx, 190.0, 60.0, 220.0);

    let close = show(ctx, "modal_delete_data", true, |ui| {
        title(ui, &format!("Delete {display}\u{2019}s data?"));
        if data.is_empty() {
            // gone after a refresh (or deleted elsewhere): nothing left to confirm
            body(ui, "There is no data left to delete.");
        } else {
            data_list(ui, data, list_h);
            if installed {
                // Nothing can be deleted yet, so no warning about it.
                body(ui, &format!("{display} is still installed and uses this data. Remove it first, then delete its data."));
            } else {
                body(ui, "This can\u{2019}t be undone.");
            }
        }
        let mut close = false;
        buttons(ui, |ui| {
            // right-to-left: the last pushed is leftmost
            if !data.is_empty() {
                let go = ui.add_enabled(state.actions_enabled() && !installed, theme::danger_button("Delete"));
                theme::hover_outline(ui, &go, theme::DANGER);
                if go.clicked() {
                    // Close first: the delete opens the progress dialog, which the close must not take away.
                    actions.push(Action::CloseModal);
                    actions.push(Action::DeleteData { name: name.clone() });
                }
            }
            let cancel = ui.add(theme::secondary_button(if data.is_empty() { "Close" } else { "Cancel" }));
            focus_first_frames(ui.ctx(), &cancel, "delete-data");
            if cancel.clicked() {
                close = true;
            }
        });
        close
    });
    if close {
        actions.push(Action::CloseModal);
    }
}

/// Hanging indent of a wrapped path, in pairs of spaces (mono: about 1 em).
const HANG: usize = 1;

/// `wrapped` (lines joined by `\n`) with `indent` in front of every line but the first, so the
/// continuation of a long path does not look like the next path.
fn hang(wrapped: &str, indent: &str) -> String {
    wrapped.split('\n').enumerate().map(|(i, l)| if i == 0 { l.to_owned() } else { format!("{indent}{l}") }).collect::<Vec<_>>().join("\n")
}

/// The data folders in a FIELD box: the path (mono, wrapped at slashes) on the left, its size dim on the right.
fn data_list(ui: &mut egui::Ui, data: &[crate::model::DataPath], max_h: f32) {
    let size_font = theme::regular(12.0);
    let sizes: Vec<String> = data.iter().map(|d| d.kb.map_or_else(|| "size unknown".to_owned(), crate::model::fmt_kb)).collect();
    let sw = sizes.iter().map(|t| ui.painter().layout_no_wrap(t.clone(), size_font.clone(), theme::TEXT_DIM).size().x).fold(0.0_f32, f32::max).ceil();
    Frame::new()
        .fill(theme::FIELD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                // more between two paths than between the wrapped lines of one
                ui.spacing_mut().item_spacing.y = 9.0;
                // 12 px between the path and its size, and a little for the scroll bar
                let pw = (ui.available_width() - sw - 12.0 - 10.0).max(60.0);
                let font = theme::mono(11.5);
                let indent = "  ".repeat(HANG);
                let hang_w = ui.painter().layout_no_wrap(indent.clone(), font.clone(), theme::TEXT).size().x;
                for (d, size) in data.iter().zip(&sizes) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let shown = hang(&theme::wrap_at_slashes(ui, &tilde(&d.path), &font, pw - hang_w), &indent);
                        ui.allocate_ui_with_layout(egui::vec2(pw, 0.0), Layout::top_down(Align::Min), |ui| {
                            ui.set_width(pw);
                            ui.add(egui::Label::new(RichText::new(shown).font(font.clone()).color(theme::TEXT)).wrap());
                        });
                        ui.add_space(12.0);
                        ui.add(egui::Label::new(RichText::new(size).font(size_font.clone()).color(theme::TEXT_DIM)).wrap_mode(egui::TextWrapMode::Extend));
                    });
                }
            });
        });
}

// ---- Run: the progress dialog (canvas boards 2a / 3 / 4) -------------------------------------

/// The status badge on an icon's corner.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Ok,
    Bad,
    Warn,
}

fn run(ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
    let Some(run) = &state.run else {
        actions.push(Action::CloseModal); // a dialog without a run: nothing to show
        return;
    };
    let close = if run.is_all() { run_all(ctx, state, run, actions) } else { run_one(ctx, state, run, actions) };
    if close {
        actions.push(Action::CloseModal);
    }
}

/// "a596443" out of "2026-10-20 a596443".
fn sha(v: &str) -> &str {
    v.split_whitespace().last().unwrap_or(v)
}

/// "Install", "Update", ...
fn capital(verb: Verb) -> String {
    let b = verb.base();
    format!("{}{}", b[..1].to_uppercase(), &b[1..])
}

/// After a Remove: what the program left behind ("2 folders, 14 MB"), once the refreshed listing is in.
fn data_left(run: &Run, entry: Option<&Entry>) -> Option<String> {
    let e = entry.filter(|e| run.verb == Verb::Remove && !e.is_installed())?;
    e.data_label()
}

type Sub = Vec<(String, Color32)>;

/// The mono line under a single run's title.
fn sub_line(run: &Run, entry: Option<&Entry>) -> Sub {
    let dim = theme::TEXT_DIM;
    let mut v: Sub = Vec::new();
    match &run.phase {
        RunPhase::Running => {
            use crate::model::Command;
            let branch = match &run.cmd {
                Command::Install { branch, .. } => branch.clone().or_else(|| entry.and_then(|e| e.branch.clone())),
                Command::Switch { branch, .. } => Some(branch.clone()),
                _ => None,
            };
            if run.verb == Verb::Update {
                if let (Some(i), Some(l)) = (entry.and_then(|e| e.installed.as_ref()), entry.and_then(|e| e.latest.as_ref())) {
                    v.push((format!("{} \u{2192} {}", i.sha, l.sha), dim));
                }
            } else if let Some(b) = branch {
                v.push((format!("branch {b}"), dim));
            }
        }
        RunPhase::Done { menu } => {
            if *menu {
                v.push(("Installed".into(), theme::GOOD));
            }
            if let Some((a, b)) = &run.change {
                v.push((format!("{} \u{2192} {}", sha(a), sha(b)), dim));
            } else if !matches!(run.verb, Verb::Remove | Verb::DeleteData) {
                if let Some(e) = entry {
                    if let Some(b) = &e.branch {
                        v.push((format!("branch {b}"), dim));
                    }
                    if let Some(i) = &e.installed {
                        v.push((i.to_string(), dim));
                    }
                }
            }
            // A removed program that left data behind: say why the "Delete data…" button is there.
            if let Some(label) = data_left(run, entry) {
                v.push((format!("Data left: {label}"), dim));
            }
        }
        RunPhase::Failed { .. } => {
            if !matches!(run.verb, Verb::Install | Verb::Remove | Verb::DeleteData) {
                if let Some(i) = entry.and_then(|e| e.installed.as_ref()) {
                    v.push((format!("still on {}", i.sha), dim));
                }
            }
        }
        _ => {}
    }
    v
}

/// The icon of a program (a letter tile when it has none); the app's own for Update all.
fn app_icon(ui: &mut egui::Ui, state: &AppState, name: Option<&str>, size: f32) -> egui::Rect {
    let entry = name.and_then(|n| state.listing.entries().find(|e| e.name == n));
    if let Some(e) = entry {
        return icons::draw(ui, e, state.dir.as_deref(), size).rect;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let r = rect.shrink(size * 0.0625);
    ui.painter().rect(
        r,
        egui::CornerRadius::same((r.width() * 0.23) as u8),
        theme::BTN,
        Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    theme::paint_glyph(ui.painter(), r.center(), glyph::REFRESH, size * 0.4, theme::ACCENT);
    rect
}

/// Canvas `.badge`: 20 px disc on the icon's bottom-right corner, 2 px PANEL ring.
fn paint_mark(ui: &egui::Ui, icon: egui::Rect, mark: Mark) {
    let p = ui.painter().with_clip_rect(ui.clip_rect().expand(12.0));
    let c = icon.right_bottom() - egui::vec2(5.0, 5.0);
    let col = match mark {
        Mark::Ok => theme::GOOD,
        Mark::Bad => theme::DANGER,
        Mark::Warn => theme::WARN,
    };
    p.circle_filled(c, 10.0, theme::PANEL);
    p.circle_filled(c, 8.0, col);
    let st = Stroke::new(2.0_f32, theme::PRIMARY_TEXT);
    match mark {
        Mark::Ok => theme::paint_glyph(&p, c, glyph::CHECK, 10.0, theme::PRIMARY_TEXT),
        Mark::Bad => {
            let d = 3.2;
            p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], st);
            p.line_segment([c + egui::vec2(d, -d), c + egui::vec2(-d, d)], st);
        }
        Mark::Warn => {
            p.line_segment([c + egui::vec2(0.0, -4.5), c + egui::vec2(0.0, 1.0)], st);
            p.circle_filled(c + egui::vec2(0.0, 4.2), 1.2, theme::PRIMARY_TEXT);
        }
    }
}

/// Icon + badge on the left, bold 16 px title and an optional mono line on the right.
/// `mono_sub`: the sub-line is a version / sha / branch (Geist Mono) rather than prose (Geist).
fn head(ui: &mut egui::Ui, state: &AppState, name: Option<&str>, size: f32, mark: Option<Mark>, title: &str, sub: &[(String, Color32)], mono_sub: bool) {
    // a block exactly as tall as the icon, so the title block centres against it (not the whole dialog)
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), size), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        let icon = app_icon(ui, state, name, size);
        if let Some(m) = mark {
            paint_mark(ui, icon, m);
        }
        let wrap = ui.available_width();
        let title_h = ui.painter().layout(title.to_owned(), theme::bold(16.0), theme::TEXT, wrap).size().y;
        let job = (!sub.is_empty()).then(|| {
            let mut job = egui::text::LayoutJob::default();
            let font = if mono_sub { theme::mono(12.0) } else { theme::regular(12.0) };
            for (i, (text, col)) in sub.iter().enumerate() {
                if i > 0 {
                    job.append(" \u{b7} ", 0.0, egui::text::TextFormat::simple(font.clone(), theme::TEXT_FAINT));
                }
                job.append(text, 0.0, egui::text::TextFormat::simple(font.clone(), *col));
            }
            job
        });
        let sub_h = job.as_ref().map_or(0.0, |j| {
            let mut j = j.clone();
            j.wrap.max_width = wrap;
            ui.painter().layout_job(j).size().y + 3.0
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add_space(((size - title_h - sub_h) / 2.0).max(0.0)); // centred against the icon
            ui.add(egui::Label::new(RichText::new(title).font(theme::bold(16.0)).color(theme::TEXT)).wrap());
            if let Some(job) = job {
                ui.add(egui::Label::new(job).wrap());
            }
        });
    });
}

fn divider(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::BORDER);
}

/// Canvas `.bar`: an indeterminate sweep (the CLI prints no percentage) on a TRACK.
fn sweep_bar(ui: &mut egui::Ui, h: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
    if ui.is_rect_visible(r) {
        let round = egui::CornerRadius::same((h / 2.0).ceil() as u8);
        ui.painter().rect_filled(r, round, TRACK);
        let seg = super::groups::sweep_rect(r, ui.ctx().input(|i| i.time));
        ui.painter().rect_filled(seg, round, theme::ACCENT);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

/// Canvas `.bar.det`: `frac` of the track filled with `color`.
fn count_bar(ui: &mut egui::Ui, h: f32, frac: f32, color: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
    let round = egui::CornerRadius::same((h / 2.0).ceil() as u8);
    ui.painter().rect_filled(r, round, TRACK);
    if frac > 0.0 {
        let fill = egui::Rect::from_min_size(r.min, egui::vec2((r.width() * frac.clamp(0.0, 1.0)).max(h), h));
        ui.painter().rect_filled(fill, round, color);
    }
}

/// A modal button: 28 px high, 14 px side padding (canvas `.btn`).
fn mbtn(ui: &mut egui::Ui, primary: bool, text: &str) -> egui::Response {
    let font = if primary { theme::bold(13.0) } else { theme::regular(13.0) };
    let w = ui.painter().layout_no_wrap(text.to_owned(), font, theme::TEXT).size().x.ceil() + 30.0;
    let b = if primary { theme::primary_button(text) } else { theme::secondary_button(text) };
    ui.add(b.min_size(egui::vec2(w, BTN_H)))
}

/// A copy button that says "Copied" for a moment after the click and keeps its width. The caller
/// pushes the action on `.clicked()`.
fn copy_btn(ui: &mut egui::Ui, key: &str, idle: &str, primary: bool) -> egui::Response {
    let copied = theme::flashing(ui.ctx(), key);
    let font = if primary { theme::bold(13.0) } else { theme::regular(13.0) };
    let w = [idle, "Copied"].iter().map(|t| ui.painter().layout_no_wrap((*t).into(), font.clone(), theme::TEXT).size().x).fold(0.0_f32, f32::max).ceil() + 30.0;
    let label = if copied { "Copied" } else { idle };
    let b = if primary { theme::primary_button(label) } else { theme::secondary_button(label) };
    let r = ui.add(b.min_size(egui::vec2(w, BTN_H)));
    if r.clicked() {
        theme::flash(ui.ctx(), key);
    }
    r
}

/// The ghost "Show log" / "Hide log" button with a chevron.
fn log_toggle(ui: &mut egui::Ui, open: bool, actions: &mut Vec<Action>) {
    let label = if open { "Hide log" } else { "Show log" };
    let tw = ui.painter().layout_no_wrap(label.into(), theme::regular(13.0), theme::TEXT_DIM).size().x;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0 + 12.0 + 6.0 + tw + 10.0, BTN_H), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let col = if resp.hovered() { theme::TEXT } else { theme::TEXT_DIM };
    if resp.hovered() {
        ui.painter().rect_stroke(rect, egui::CornerRadius::same(theme::RADIUS), Stroke::new(1.0_f32, theme::BTNBD), egui::StrokeKind::Inside);
    }
    theme::paint_glyph(ui.painter(), egui::pos2(rect.left() + 10.0 + 6.0, rect.center().y), if open { CHEVRON_UP } else { glyph::CHEVRON }, 9.5, col);
    ui.painter().text(egui::pos2(rect.left() + 10.0 + 12.0 + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::regular(13.0), col);
    super::groups::focus_ring(ui, &resp, rect, theme::RADIUS);
    if resp.clicked() {
        actions.push(Action::ToggleRunLog);
    }
}

/// `left` on the left, `right` right-aligned (added right-to-left: the first one is the rightmost).
fn button_row(ui: &mut egui::Ui, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::GAP;
        left(ui);
        let w = ui.available_width();
        ui.allocate_ui_with_layout(egui::vec2(w, BTN_H), Layout::right_to_left(Align::Center), right);
    });
}

/// The `[Show log] [Copy log]` group on the left of a button row.
fn log_buttons(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    log_toggle(ui, state.run_log_open, actions);
    if state.run_log_open && copy_btn(ui, "run_copy_log", "Copy log", false).clicked() {
        actions.push(Action::CopyLog);
    }
}

/// The run's newest lines in a log box.
fn run_log(ui: &mut egui::Ui, state: &AppState, h: f32) {
    let lines = state.run_lines();
    let lines = &lines[lines.len().saturating_sub(super::logview::RUN_LINES)..];
    let serial = state.run.as_ref().map_or(0, |r| r.log_start());
    super::logview::show(ui, &format!("run{serial}"), lines, ui.available_width(), h, false, state.touch_mode);
}

/// A mono line in a FIELD box (canvas `.code`).
fn mono_box(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(theme::FIELD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).font(theme::mono(12.0)).color(color)).wrap());
        });
}

/// Room for a list / log in a dialog that already has `used` px of other content.
fn room(ctx: &egui::Context, used: f32, min: f32, max: f32) -> f32 {
    (ctx.content_rect().height() - 2.0 * SCREEN_PAD - used).clamp(min, max)
}

#[derive(Clone, Copy)]
enum Title {
    Running,
    Done,
    Failed,
}

/// The progress dialog's title: "Installing Ritz…", "Installed Ritz", "Couldn’t install Ritz". Deleting
/// data reads "Deleting Ritz’s data…" (the verb's own wording is for the log and the status bar).
fn run_title(verb: Verb, kind: Title, display: &str) -> String {
    if verb == Verb::DeleteData {
        return match kind {
            Title::Running => format!("Deleting {display}\u{2019}s data\u{2026}"),
            Title::Done => format!("Deleted {display}\u{2019}s data"),
            Title::Failed => format!("Couldn\u{2019}t delete {display}\u{2019}s data"),
        };
    }
    match kind {
        Title::Running => format!("{} {display}\u{2026}", verb.gerund()),
        Title::Done => format!("{} {display}", verb.past()),
        Title::Failed => format!("Couldn\u{2019}t {} {display}", verb.base()),
    }
}

/// The line under a failure's reason.
fn failed_note(verb: Verb, reason: &str, display: &str) -> String {
    match verb {
        Verb::Install => "Nothing was installed. Open the log for details, then try again.".to_owned(),
        Verb::Remove => "Open the log for details, then try again.".to_owned(),
        // ritz-get refuses to delete the data of an installed program: `uninstall first: ritz-get remove NAME`
        Verb::DeleteData if reason.starts_with("uninstall first") => format!("Nothing was deleted. Remove {display} first, then delete its data."),
        Verb::DeleteData => "Nothing was deleted. Open the log for details, then try again.".to_owned(),
        _ => "Your installed version is unchanged and still works.".to_owned(),
    }
}

fn run_one(ctx: &egui::Context, state: &AppState, run: &Run, actions: &mut Vec<Action>) -> bool {
    let name = run.name.as_deref().unwrap_or_default();
    let entry = state.listing.entries().find(|e| e.name == name);
    let display = state.display_of(name);
    let verb = run.verb;
    let menu = matches!(run.phase, RunPhase::Done { menu: true });

    let (mark, title) = match &run.phase {
        RunPhase::Running => (None, run_title(verb, Title::Running, &display)),
        RunPhase::Done { menu: true } => (Some(Mark::Ok), display.clone()),
        RunPhase::Done { menu: false } => (Some(Mark::Ok), run_title(verb, Title::Done, &display)),
        RunPhase::Failed { .. } => (Some(Mark::Bad), run_title(verb, Title::Failed, &display)),
        RunPhase::Deps { .. } => (Some(Mark::Warn), "Missing tools".to_owned()),
        RunPhase::Setup => (Some(Mark::Warn), "Needs setup in a terminal".to_owned()),
        RunPhase::Busy => (Some(Mark::Warn), "ritz-get is busy".to_owned()),
    };
    let sub = sub_line(run, entry);
    let log_h = room(ctx, 330.0, 90.0, 150.0);
    let tag = match &run.phase {
        RunPhase::Running => "run-running",
        RunPhase::Done { .. } => "run-done",
        RunPhase::Failed { .. } => "run-failed",
        _ => "run-other",
    };

    show_with(ctx, "modal_run", RUN_W, RUN_PAD, RUN_GAP, true, |ui| {
        head(ui, state, Some(name), 56.0, mark, &title, &sub, true);
        let mut close = false;
        match &run.phase {
            RunPhase::Running => {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    sweep_bar(ui, 6.0);
                    let line = state.run_latest().map_or("Starting\u{2026}", |l| l.text.as_str());
                    ui.add(egui::Label::new(RichText::new(line).font(theme::mono(12.0)).color(theme::TEXT)).wrap());
                });
                if state.run_log_open {
                    run_log(ui, state, log_h);
                }
                button_row(ui, |ui| log_buttons(ui, state, actions), |ui| {
                    let bg = mbtn(ui, false, "Run in background");
                    focus_first_frames(ui.ctx(), &bg, tag);
                    close |= bg.clicked();
                });
            }
            RunPhase::Done { .. } if menu => {
                divider(ui);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.add(egui::Label::new(RichText::new("Add it to the start menu?").font(theme::bold(15.0)).color(theme::TEXT)).wrap());
                    body(
                        ui,
                        "Puts a launcher in your desktop\u{2019}s application menu, so you can open it without ritz-get-gui. \
                         You can change this later in the program\u{2019}s actions.",
                    );
                });
                menu_preview(ui, state, entry, &display);
                button_row(ui, |_| {}, |ui| {
                    let add = ui.add_enabled(state.actions_enabled(), theme::primary_button("Add to start menu").min_size(egui::vec2(0.0, BTN_H)));
                    focus_first_frames(ui.ctx(), &add, tag);
                    if add.clicked() {
                        actions.push(Action::Shortcut { name: name.to_owned(), remove: false });
                        close = true;
                    }
                    close |= mbtn(ui, false, "Not now").clicked();
                });
            }
            RunPhase::Done { .. } => {
                if let Some(l) = state.run_lines().iter().rev().find(|l| l.kind == LogKind::Ok) {
                    mono_box(ui, &l.text, theme::GOOD);
                }
                if state.run_log_open {
                    run_log(ui, state, log_h);
                }
                let has_data = data_left(run, entry).is_some();
                let mut more = Vec::new();
                button_row(ui, |ui| log_buttons(ui, state, actions), |ui| {
                    let done = mbtn(ui, true, "Done");
                    focus_first_frames(ui.ctx(), &done, tag);
                    close |= done.clicked();
                    // Only opens the confirmation; nothing is deleted from here.
                    if has_data && mbtn(ui, false, "Delete data\u{2026}").clicked() {
                        if let Some(e) = entry {
                            more.push(Action::CloseModal);
                            more.push(Action::ShowModal(Modal::DeleteData { id: e.id() }));
                        }
                    }
                });
                actions.extend(more);
            }
            RunPhase::Failed { reason } => {
                alert(ui, reason);
                body(ui, &failed_note(verb, reason, &display));
                if state.run_log_open {
                    run_log(ui, state, log_h);
                }
                button_row(ui, |ui| log_buttons(ui, state, actions), |ui| {
                    let c = mbtn(ui, false, "Close");
                    focus_first_frames(ui.ctx(), &c, tag);
                    close |= c.clicked();
                });
            }
            RunPhase::Deps { missing, command } => {
                let tools = tool_names(missing);
                let intro = format!("{display} can\u{2019}t be built yet: {}.", missing_sentence(&tools));
                close |= deps_block(ui, &intro, &capital(verb), missing, command.as_deref(), actions);
            }
            RunPhase::Setup => {
                body(ui, &format!("{display} needs a system package that only a terminal install can set up. Run this in a terminal, then press {} again.", capital(verb)));
                let cmd = format!("ritz-get install {name}");
                code_box(ui, &cmd);
                button_row(ui, |_| {}, |ui| {
                    let copy = copy_btn(ui, "modal_copy_command", "Copy command", true);
                    focus_first_frames(ui.ctx(), &copy, tag);
                    if copy.clicked() {
                        actions.push(Action::CopyToClipboard(cmd.clone()));
                    }
                    close |= mbtn(ui, false, "Close").clicked();
                });
            }
            RunPhase::Busy => {
                body(ui, "Another ritz-get is running. Wait for it to finish, then try again.");
                button_row(ui, |_| {}, |ui| {
                    let c = mbtn(ui, true, "Close");
                    focus_first_frames(ui.ctx(), &c, tag);
                    close |= c.clicked();
                });
            }
        }
        close
    })
}

/// 2a: a preview of the application-menu entry (canvas: FIELD box, one SEL row: 28 px icon + name).
fn menu_preview(ui: &mut egui::Ui, state: &AppState, entry: Option<&Entry>, display: &str) {
    Frame::new()
        .fill(theme::FIELD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            Frame::new()
                .fill(theme::SEL)
                .stroke(Stroke::new(1.0_f32, theme::SELBD))
                .corner_radius(theme::RADIUS)
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        match entry {
                            Some(e) => {
                                icons::draw(ui, e, state.dir.as_deref(), 28.0);
                            }
                            None => {
                                ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
                            }
                        }
                        ui.add(egui::Label::new(RichText::new(display).font(theme::regular(13.0)).color(theme::TEXT)).truncate());
                    });
                });
        });
}

/// The failure: a FAILED pill and the reason, on a faint red tint.
fn alert(ui: &mut egui::Ui, reason: &str) {
    Frame::new()
        .fill(theme::with_alpha(theme::DANGER, 0.10))
        .stroke(Stroke::new(1.0_f32, theme::with_alpha(theme::DANGER, 0.30)))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                theme::pill(ui, "Failed", theme::DANGER);
                ui.add(egui::Label::new(RichText::new(reason).font(theme::mono(12.0)).color(DANGER_T)).wrap());
            });
        });
}

/// Exit 3: what is missing and how to get it. Returns whether to close.
fn deps_block(ui: &mut egui::Ui, intro: &str, again: &str, missing: &[String], command: Option<&str>, actions: &mut Vec<Action>) -> bool {
    let mut close = false;
    body(ui, intro);
    match command {
        Some(cmd) => {
            body(ui, &format!("Run this in a terminal, then press {again} again."));
            code_box(ui, cmd);
        }
        None => {
            body(ui, &format!("Install these with your package manager, then press {again} again."));
            code_box(ui, &missing.join("\n"));
            body(ui, "Not sure how? Send this to whoever set up your PC.");
        }
    }
    button_row(ui, |_| {}, |ui| match command {
        Some(cmd) => {
            let copy = copy_btn(ui, "modal_copy_command", "Copy command", true);
            focus_first_frames(ui.ctx(), &copy, "deps");
            if copy.clicked() {
                actions.push(Action::CopyToClipboard(cmd.to_owned()));
            }
            close |= mbtn(ui, false, "Close").clicked();
        }
        None => {
            // No command to copy: offer the raw `missing:` lines (e.g. to send to whoever set up the PC).
            let closed = mbtn(ui, false, "Close");
            focus_first_frames(ui.ctx(), &closed, "deps");
            close |= closed.clicked();
            if copy_btn(ui, "modal_copy_missing", "Copy", false).clicked() {
                actions.push(Action::CopyToClipboard(missing.join("\n")));
            }
        }
    });
    close
}

// ---- Update all (canvas board 4) ---------------------------------------------------------------

const ROW_H: f32 = 58.0;

/// (pill text, pill colour, mono line, line colour, bar) of one Update-all row.
fn row_view(state: &AppState, name: &str, cp: &CardProgress) -> (&'static str, Color32, String, Color32, RowBar) {
    let dim = theme::TEXT_DIM;
    match &cp.phase {
        Progress::Queued => ("Queued", dim, "Waiting".into(), dim, RowBar::Empty),
        Progress::Running => {
            let line = state.run_latest().map_or_else(|| "Starting\u{2026}".to_owned(), |l| l.text.clone());
            ("Updating\u{2026}", theme::ACCENT, line, theme::TEXT, RowBar::Sweep)
        }
        Progress::Done => ("Done", theme::GOOD, "Finished".into(), dim, RowBar::Full(theme::GOOD)),
        Progress::Failed => {
            let prefix = format!("{name}: FAILED (");
            let reason = state
                .run_lines()
                .iter()
                .rev()
                .find_map(|l| l.text.strip_prefix(&prefix).map(|r| r.strip_suffix(')').unwrap_or(r).to_owned()))
                .unwrap_or_else(|| "it failed".into());
            ("Failed", theme::DANGER, reason, DANGER_T, RowBar::Full(theme::DANGER))
        }
        Progress::Result(o) => match o {
            Outcome::Updated { from, to } => ("Updated", theme::GOOD, format!("{} \u{2192} {}", sha(from), sha(to)), dim, RowBar::Full(theme::GOOD)),
            Outcome::UpToDate => ("Up to date", dim, "Already up to date".into(), dim, RowBar::Full(BAR_DIM)),
            Outcome::Held => ("Skipped", dim, "Auto-update is off for this program".into(), dim, RowBar::Empty),
            Outcome::SkippedLocalChanges => ("Skipped", theme::WARN, "Local changes kept; reset it to discard them".into(), dim, RowBar::Empty),
            Outcome::SkippedNeedsNewer => ("Skipped", theme::WARN, "Needs a newer ritz-get".into(), dim, RowBar::Empty),
            Outcome::Failed { reason } => ("Failed", theme::DANGER, reason.clone(), DANGER_T, RowBar::Full(theme::DANGER)),
        },
    }
}

#[derive(Clone, Copy)]
enum RowBar {
    Empty,
    Sweep,
    Full(Color32),
}

/// Draw into `rect` without touching the parent's cursor (a `scope_builder` would move it back up
/// to the end of that rect).
fn child(ui: &mut egui::Ui, rect: egui::Rect, layout: Layout, add: impl FnOnce(&mut egui::Ui)) {
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    add(&mut c);
}

fn update_row(ui: &mut egui::Ui, state: &AppState, name: &str, cp: &CardProgress, first: bool, w: f32) {
    let (pill, pill_col, line, line_col, bar) = row_view(state, name, cp);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let running = cp.phase == Progress::Running;
    if running {
        ui.painter().rect_filled(rect, 0.0, theme::SEL);
        // keep the running program in view (once per program, not every frame)
        let key = Id::new("run_current_row");
        if ui.ctx().data_mut(|d| d.get_temp::<String>(key)).as_deref() != Some(name) {
            ui.ctx().data_mut(|d| d.insert_temp(key, name.to_owned()));
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
    }
    if !first {
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, theme::BORDER));
    }
    let x0 = rect.left() + 12.0;
    let icon_rect = egui::Rect::from_min_size(egui::pos2(x0, rect.top() + 8.0), egui::vec2(32.0, 32.0));
    let entry = state.listing.entries().find(|e| e.name == name);
    child(ui, icon_rect, Layout::top_down(Align::Min), |ui| match entry {
        Some(e) => {
            icons::draw(ui, e, state.dir.as_deref(), 32.0);
        }
        None => {
            app_icon(ui, state, None, 32.0);
        }
    });
    // the pill, right-aligned on the icon's centre line
    let pill_zone = egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 8.0), egui::pos2(rect.right() - 12.0, rect.top() + 40.0));
    let mut pill_left = pill_zone.right();
    child(ui, pill_zone, Layout::right_to_left(Align::Center), |ui| {
        pill_left = theme::pill(ui, pill, pill_col).rect.left();
    });
    let tx = x0 + 32.0 + 10.0;
    let tw = (pill_left - 10.0 - tx).max(20.0);
    let text_rect = |y: f32, h: f32| egui::Rect::from_min_size(egui::pos2(tx, rect.top() + y), egui::vec2(tw, h));
    let display = state.display_of(name);
    child(ui, text_rect(7.0, 17.0), Layout::left_to_right(Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(display).font(theme::bold(13.0)).color(theme::TEXT)).truncate());
    });
    child(ui, text_rect(24.0, 16.0), Layout::left_to_right(Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(line).font(theme::mono(11.5)).color(line_col)).truncate());
    });
    // its own thin bar
    let track = egui::Rect::from_min_size(egui::pos2(x0, rect.bottom() - 8.0 - 3.0), egui::vec2(rect.width() - 24.0, 3.0));
    ui.painter().rect_filled(track, 2.0, TRACK);
    match bar {
        RowBar::Empty => {}
        RowBar::Sweep => {
            ui.painter().rect_filled(super::groups::sweep_rect(track, ui.ctx().input(|i| i.time)), 2.0, theme::ACCENT);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        RowBar::Full(c) => {
            ui.painter().rect_filled(track, 2.0, c);
        }
    }
}

/// "2 updated, 1 up to date, 1 skipped, 1 failed" over the rows (zero parts left out).
pub(crate) fn tally(state: &AppState) -> (String, usize) {
    let Some(run) = &state.run else { return (String::new(), 0) };
    let (mut updated, mut even, mut skipped, mut failed, mut done) = (0, 0, 0, 0, 0);
    for cp in run.order.iter().filter_map(|n| state.progress.get(n)) {
        match &cp.phase {
            Progress::Result(Outcome::Updated { .. }) => updated += 1,
            Progress::Result(Outcome::UpToDate) => even += 1,
            Progress::Result(_) => skipped += 1,
            Progress::Failed => failed += 1,
            Progress::Done => done += 1,
            Progress::Queued | Progress::Running => {}
        }
    }
    let parts: Vec<String> = [(updated, "updated"), (done, "done"), (even, "up to date"), (skipped, "skipped"), (failed, "failed")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect();
    (parts.join(", "), failed)
}

fn run_all(ctx: &egui::Context, state: &AppState, run: &Run, actions: &mut Vec<Action>) -> bool {
    let (done, total) = state.run_counts();
    let (summary, failed) = tally(state);
    let running = run.running();
    let counted = state.run_counted();
    let mark = if running {
        None
    } else if failed == 0 && matches!(run.phase, RunPhase::Done { .. }) {
        Some(Mark::Ok)
    } else {
        Some(Mark::Warn)
    };
    let title = if running { state.run_all_title() } else { "Update finished".to_owned() };
    let sub = if running && counted {
        format!("{done} of {total} finished")
    } else if running && state.marks.started {
        "Checked; updating now".to_owned()
    } else if running {
        "Looking for newer versions".to_owned()
    } else if summary.is_empty() {
        "Nothing to update".to_owned()
    } else {
        summary.clone()
    };
    let failed_names: Vec<String> = run
        .order
        .iter()
        .filter(|n| state.progress.get(*n).is_some_and(|c| c.phase == Progress::Failed || matches!(c.phase, Progress::Result(Outcome::Failed { .. }))))
        .map(|n| state.display_of(n))
        .collect();
    let extra_h = match &run.phase {
        RunPhase::Deps { .. } => 200.0,
        _ => 40.0 + if failed_names.is_empty() { 0.0 } else { 30.0 },
    };
    let log_h = 110.0;
    let used = 40.0 + 48.0 + 6.0 + BTN_H + 3.0 * 14.0 + extra_h + if state.run_log_open { log_h + 14.0 } else { 0.0 } + 14.0;
    let rows_h = room(ctx, used, ROW_H * 2.0, ROW_H * 12.0);
    let tag = if running { "all-running" } else { "all-done" };

    show_with(ctx, "modal_run", ALL_W, RUN_PAD, 14.0, true, |ui| {
        head(ui, state, Some("ritz-get-gui"), 48.0, mark, &title, &[(sub.clone(), theme::TEXT_DIM)], false);
        let bar_col = if running { theme::ACCENT } else if failed > 0 { theme::WARN } else { theme::GOOD };
        if running && !counted {
            sweep_bar(ui, 6.0); // ritz-get has not said how many yet
        } else {
            count_bar(ui, 6.0, if total == 0 { 0.0 } else { done as f32 / total as f32 }, bar_col);
        }

        if total > 0 {
            Frame::new().stroke(Stroke::new(1.0_f32, theme::BORDER)).corner_radius(theme::RADIUS).show(ui, |ui| {
                let w = ui.available_width();
                super::vscroll(state.touch_mode).id_salt("run_rows").max_height(rows_h).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut first = true;
                    for name in &run.order {
                        if let Some(cp) = state.progress.get(name) {
                            update_row(ui, state, name, cp, first, w - 2.0);
                            first = false;
                        }
                    }
                });
            });
        }

        let mut close = false;
        if !running {
            if !failed_names.is_empty() {
                let one = failed_names.len() == 1;
                body(
                    ui,
                    &format!(
                        "{} failed. {} installed version{} unchanged and still work{}.",
                        failed_names.join(", "),
                        if one { "Its" } else { "Their" },
                        if one { " is" } else { "s are" },
                        if one { "s" } else { "" },
                    ),
                );
            }
            match &run.phase {
                RunPhase::Failed { reason } if failed_names.is_empty() => body(ui, &format!("The update stopped: {reason}.")),
                RunPhase::Busy => body(ui, "Another ritz-get is running. Wait for it to finish, then try again."),
                RunPhase::Setup => body(ui, "Some programs need setup in a terminal. The log has the command for each."),
                _ => {}
            }
        }
        if let (false, RunPhase::Deps { missing, command }) = (running, &run.phase) {
            let tools = tool_names(missing);
            let intro = format!("Some programs can\u{2019}t be built yet: {}.", missing_sentence(&tools));
            close |= deps_block(ui, &intro, "Update all", missing, command.as_deref(), actions);
            if state.run_log_open {
                run_log(ui, state, log_h);
            }
            return close;
        }
        if state.run_log_open {
            run_log(ui, state, log_h);
        }
        button_row(ui, |ui| log_buttons(ui, state, actions), |ui| {
            if running {
                let bg = mbtn(ui, false, "Run in background");
                focus_first_frames(ui.ctx(), &bg, tag);
                close |= bg.clicked();
            } else {
                let c = mbtn(ui, true, "Close");
                focus_first_frames(ui.ctx(), &c, tag);
                close |= c.clicked();
            }
        });
        close
    })
}

// ---- Log dialog --------------------------------------------------------------------------------

fn log_modal(ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) {
    let h = room(ctx, 130.0, 120.0, 380.0);
    let close = show_with(ctx, "modal_log", LOG_W, MODAL_PAD, theme::GAP, true, |ui| {
        ui.horizontal(|ui| {
            let (g, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), Sense::hover());
            theme::paint_glyph(ui.painter(), g.center(), glyph::TERMINAL, 12.0, theme::TEXT_DIM);
            title(ui, "Log");
            let n = state.log.len();
            ui.add(egui::Label::new(RichText::new(format!("{n} {}", if n == 1 { "line" } else { "lines" })).font(theme::mono(11.0)).color(theme::TEXT_FAINT)));
        });
        super::logview::show(ui, "full", &state.log, ui.available_width(), h, true, state.touch_mode);
        let mut close = false;
        buttons(ui, |ui| {
            let c = ui.add(theme::primary_button("Close"));
            focus_first_frames(ui.ctx(), &c, "log");
            close |= c.clicked();
            if copy_btn(ui, "log_copy", "Copy log", false).clicked() {
                actions.push(Action::CopyLog);
            }
        });
        close
    });
    if close {
        actions.push(Action::CloseModal);
    }
}

/// Tool names out of `missing: TOOL (try: ...)` lines (a line without the prefix is used whole).
/// "cargo is missing", "cargo and pkg-config are missing", "a, b and c are missing".
pub(crate) fn missing_sentence(tools: &[String]) -> String {
    match tools {
        [] => "some tools are missing".to_owned(),
        [one] => format!("{one} is missing"),
        [init @ .., last] => format!("{} and {last} are missing", init.join(", ")),
    }
}

pub(crate) fn tool_names(missing: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in missing {
        let l = line.trim();
        let l = l.strip_prefix("missing:").unwrap_or(l).trim();
        let tool = l.split(" (").next().unwrap_or(l).trim();
        if !tool.is_empty() && !out.iter().any(|t| t == tool) {
            out.push(tool.to_owned());
        }
    }
    out
}

// ---- Porcelain refused -------------------------------------------------------------------------

fn refused(ctx: &egui::Context, state: &AppState, n: u32, actions: &mut Vec<Action>) {
    show(ctx, "modal_refused", false, |ui| {
        title(ui, "ritz-get-gui needs an update");
        body(ui, &format!("Your ritz-get is newer than this version of ritz-get-gui can handle (porcelain {n})."));
        body(ui, "Update ritz-get-gui to fix this. Nothing else works until then.");
        buttons(ui, |ui| {
            // Not `actions_enabled()`: that is false while refused, and this button is the fix.
            let upd = ui.add_enabled(!state.busy && !state.ritz_get_missing, theme::primary_button("Update ritz-get-gui"));
            focus_first_frames(ui.ctx(), &upd, "refused");
            if upd.clicked() {
                actions.push(Action::Update { name: Some("ritz-get-gui".into()) });
            }
        });
        false
    });
    // Deliberately no Esc / backdrop close: there is no state to dismiss and the app is unusable.
}

// ---- Banner ------------------------------------------------------------------------------------

/// The top banner "ritz-get not found" (+ Retry -> `Action::Retry`). `ui::draw` only calls this when
/// `state.ritz_get_missing`.
pub fn draw_banner(ui: &mut egui::Ui, _state: &AppState, actions: &mut Vec<Action>) {
    const LINE_H: f32 = 18.0;
    const REPO_URL: &str = "https://github.com/Ritze03/ritz-get";
    let frame = Frame::new()
        .fill(theme::with_alpha(theme::DANGER, 0.16))
        .stroke(Stroke::new(1.0_f32, theme::with_alpha(theme::DANGER, 0.42)))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::same(theme::CARD_PAD + 2));
    theme::framed(ui, frame, |ui| {
        const ICON_W: f32 = 18.0;
        const BTN_W: f32 = 62.0;
        let w = ui.available_width();
        let text_w = (w - ICON_W - BTN_W - 2.0 * theme::GAP).max(60.0);

        // Text column: bold title, then a wrapped line with a clickable link. The row height is
        // what the text needed last frame (the panel sizes itself from last frame's content, so
        // when a resize changes the wrap, draw once more to catch up).
        let h_id = Id::new("banner_row_h");
        let prev = ui.ctx().data_mut(|d| d.get_temp::<f32>(h_id)).unwrap_or(2.0 * LINE_H);
        let row_h = prev.max(theme::CTRL_H);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), Sense::hover());
        let text_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + ICON_W + theme::GAP, rect.top()),
            egui::vec2(text_w, 1000.0),
        );
        let mut col = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::top_down(Align::Min)));
        col.set_clip_rect(col.clip_rect().intersect(rect));
        col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        col.spacing_mut().interact_size.y = LINE_H;
        let line = |t: &str| RichText::new(t.to_owned()).font(theme::regular(12.0)).color(theme::TEXT_DIM).line_height(Some(LINE_H));
        col.add(egui::Label::new(RichText::new("ritz-get not found").font(theme::bold(13.0)).color(theme::TEXT).line_height(Some(LINE_H))).wrap());
        col.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.spacing_mut().interact_size.y = LINE_H;
            ui.label(line("ritz-get isn't installed (or can't be found). Install it from "));
            let link = ui.add(egui::Link::new(
                RichText::new("github.com/Ritze03/ritz-get").font(theme::regular(12.0)).color(theme::ACCENT).line_height(Some(LINE_H)),
            ));
            if link.clicked() {
                actions.push(Action::OpenUrl(REPO_URL.to_owned()));
            }
            ui.label(line(", then press Retry."));
        });
        let content_h = (col.min_rect().height()).max(theme::CTRL_H);
        if (content_h - prev).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(h_id, content_h));
            ui.ctx().request_repaint();
        }
        if ui.is_rect_visible(rect) {
            let first_line = 9.0; // centre of the 18 px title line
            theme::paint_glyph(
                ui.painter(),
                egui::pos2(rect.left() + ICON_W * 0.5, rect.top() + first_line + 1.0),
                glyph::WARNING,
                13.0,
                theme::DANGER,
            );
        }
        let btn = egui::Rect::from_min_size(
            egui::pos2(rect.right() - BTN_W, rect.center().y - theme::CTRL_H * 0.5),
            egui::vec2(BTN_W, theme::CTRL_H),
        );
        if ui.put(btn, theme::secondary_button("Retry")).clicked() {
            actions.push(Action::Retry);
        }
    });
}

// ---- Path display ------------------------------------------------------------------------------

/// `path` with a leading `$HOME` shown as `~`.
fn tilde(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(home.trim_end_matches('/')) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

// ---- Shared pieces -----------------------------------------------------------------------------

/// A centred modal. `content` returns true to close; Esc and a click on the backdrop also close
/// when `dismissible`. Returns whether to close.
fn show(ctx: &egui::Context, salt: &str, dismissible: bool, content: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    show_with(ctx, salt, MODAL_W, MODAL_PAD, theme::GAP, dismissible, content)
}

/// [`show`] with its own width, padding and gap between blocks (the progress dialogs).
fn show_with(ctx: &egui::Context, salt: &str, width: f32, pad: i8, gap: f32, dismissible: bool, content: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let outer = width.min((ctx.content_rect().width() - 2.0 * SCREEN_PAD).max(200.0));
    let frame = Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::same(pad))
        .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: shade() });
    let resp = egui::Modal::new(Id::new(salt)).backdrop_color(shade()).frame(frame).show(ctx, |ui| {
        ui.set_width(outer - 2.0 * f32::from(pad) - 2.0);
        ui.spacing_mut().item_spacing = egui::vec2(theme::GAP, gap);
        content(ui)
    });
    let by_user = dismissible && resp.should_close();
    resp.inner || by_user
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::bold(13.0)).color(theme::TEXT)).wrap());
}

fn body_text(text: &str) -> RichText {
    RichText::new(text).font(theme::regular(12.0)).color(theme::TEXT_DIM).line_height(Some(18.0))
}

fn body(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(body_text(text)).wrap());
}

/// Selectable read-only Geist Mono box (FIELD fill from the theme's `extreme_bg_color`).
fn code_box(ui: &mut egui::Ui, text: &str) {
    let mut shown: &str = text;
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .font(theme::mono(12.0))
            .desired_width(ui.available_width())
            .desired_rows(1)
            .margin(Margin::symmetric(8, 6)),
    );
}

/// Right-aligned button row (mockup `.btns`, 4 px extra top margin). Buttons are added
/// right-to-left: the first one pushed is the rightmost.
fn buttons(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0 - theme::GAP.min(4.0));
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

/// Give `resp` the keyboard focus for the first few frames of a modal state (a new `Area` is laid
/// out invisibly on its first pass, which can swallow a one-shot `request_focus`). `tag` names the
/// state (running, done, ...): when it changes, the new state's button gets the focus. Cleared by
/// [`draw`] when no modal is open.
fn focus_first_frames(ctx: &egui::Context, resp: &egui::Response, tag: &str) {
    let key = Id::new("modal_focus_frames");
    let tag = Id::new(tag).value();
    let (seen, n) = ctx.data_mut(|d| d.get_temp::<(u64, u8)>(key)).unwrap_or((tag, 0));
    let n = if seen == tag { n } else { 0 };
    if n < 3 {
        resp.request_focus();
        ctx.data_mut(|d| d.insert_temp(key, (tag, n + 1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_render;

    #[test]
    fn missing_sentence_is_natural() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(missing_sentence(&v(&[])), "some tools are missing");
        assert_eq!(missing_sentence(&v(&["cargo"])), "cargo is missing");
        assert_eq!(missing_sentence(&v(&["cargo", "pkg-config"])), "cargo and pkg-config are missing");
        assert_eq!(missing_sentence(&v(&["a", "b", "c"])), "a, b and c are missing");
    }

    #[test]
    fn tool_names_parses_missing_lines() {
        let m = vec![
            "missing: cargo (try: pacman:rust dnf:cargo apt:cargo)".to_owned(),
            "missing: pkg-config (try: pacman:pkgconf)".to_owned(),
            "missing: cargo (try: x)".to_owned(),
            "gcc".to_owned(),
        ];
        assert_eq!(tool_names(&m), ["cargo", "pkg-config", "gcc"]);
    }

    /// Every modal state renders at the three test widths (and 1.5 / 2.0 scale) and stays on screen.
    #[test]
    fn modals_render_inside_the_window() {
        use egui::Rect;
        let hooks = [
            "reset=Software/geofinder",
            "deletedata=Software/ritz",
            "deletedata=Software/SteamAlt",
            "install-running=Software/ritz",
            "install-done=Software/ritz",
            "install-failed=Software/ritz",
            "install-deps=Software/ForzaTelemetryV3Launcher",
            "deps-none=Software/ForzaTelemetryV3Launcher",
            "updateall-running",
            "updateall-done",
            "log",
        ];
        let mut states: Vec<(String, AppState)> = Vec::new();
        for h in hooks {
            let mut s = AppState { listing: crate::sample::listing(), dir: Some("/home/u/ritz-get".into()), ..AppState::default() };
            s.apply_shot_hook(h);
            states.push((h.to_owned(), s));
            if h.starts_with("install-") || h.starts_with("updateall") {
                let mut s2 = AppState { listing: crate::sample::listing(), ..AppState::default() };
                s2.apply_shot_hook(h);
                s2.run_log_open = true;
                for i in 0..40 {
                    s2.push_log(LogKind::Info, format!("line {i}: {}", "x".repeat(i * 7)));
                }
                states.push((format!("{h}+log"), s2));
            }
        }
        let mut s = AppState::default();
        s.refused = Some(2);
        states.push(("refused".into(), s));

        for (name, state) in &states {
            for ppp in [1.0, 2.0] {
                for (w, h) in test_render::SIZES {
                    let ctx = test_render::context_at(ppp);
                    let (out, _) = test_render::run_ctx(&ctx, w, h, |ctx| {
                        let mut actions = Vec::new();
                        draw(ctx, state, &mut actions);
                    });
                    let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
                    let shown = test_render::visible_rects(&out);
                    assert!(!shown.is_empty(), "{name} @ {w}: nothing drawn");
                    for r in shown {
                        assert!(screen.expand(1.0).contains_rect(r), "{name} @ {w}x{h} ppp {ppp}: {r:?} outside {screen:?}");
                    }
                }
            }
        }
    }

    /// Run `state`'s modal for a few frames, then click the button labelled `label` (or press Enter
    /// when `None`); the actions the dialog pushed on the frames after that.
    fn press(state: &AppState, label: Option<&str>) -> Vec<Action> {
        let ctx = test_render::context();
        let (w, h) = (900.0, 700.0);
        let mut actions = Vec::new();
        let mut pos = None;
        let mut events: Vec<egui::Event> = Vec::new();
        for frame in 0..14 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
                events: std::mem::take(&mut events),
                ..Default::default()
            };
            let mut found = None;
            let out = ctx.run(input, |ctx| draw(ctx, state, &mut actions));
            for cs in &out.shapes {
                if let (egui::Shape::Text(t), Some(l)) = (&cs.shape, label) {
                    if t.galley.text() == l {
                        found = Some(t.galley.rect.translate(t.pos.to_vec2()).center());
                    }
                }
            }
            match frame {
                5 => match label {
                    Some(l) => {
                        let p = found.unwrap_or_else(|| panic!("button {l:?} is not drawn"));
                        pos = Some(p);
                        events = vec![egui::Event::PointerMoved(p)];
                    }
                    None => {
                        let key = |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
                        events = vec![key(true), key(false)];
                    }
                },
                6 | 7 if pos.is_some() => {
                    events = vec![egui::Event::PointerButton { pos: pos.unwrap(), button: egui::PointerButton::Primary, pressed: frame == 6, modifiers: Default::default() }];
                }
                _ => {}
            }
        }
        actions
    }

    fn sample_state() -> AppState {
        AppState { listing: crate::sample::listing(), ..AppState::default() }
    }

    fn ritz_mut(s: &mut AppState) -> &mut Entry {
        s.listing.sections.iter_mut().flat_map(|x| x.entries.iter_mut()).find(|e| e.name == "ritz").unwrap()
    }

    /// Delete is one deliberate click: Cancel / Enter (Cancel has the focus) only close; only Delete
    /// closes and starts the delete.
    #[test]
    fn delete_data_confirm_never_deletes_by_default() {
        let mut s = sample_state();
        s.apply_shot_hook("deletedata=Software/ritz");
        assert_eq!(press(&s, Some("Cancel")), vec![Action::CloseModal]);
        assert_eq!(press(&s, None), vec![Action::CloseModal], "Enter lands on Cancel");
        assert_eq!(press(&s, Some("Delete")), vec![Action::CloseModal, Action::DeleteData { name: "ritz".into() }]);
    }

    /// Every text the modal draws (a few frames in, so the layout has settled).
    fn drawn_texts(state: &AppState) -> Vec<String> {
        let ctx = test_render::context();
        let mut actions = Vec::new();
        let mut texts = Vec::new();
        for _ in 0..6 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| draw(ctx, state, &mut actions));
            texts = out.shapes.iter().filter_map(|c| if let egui::Shape::Text(t) = &c.shape { Some(t.galley.text().to_owned()) } else { None }).collect();
        }
        texts
    }

    /// A program that is still installed cannot be confirmed (ritz-get would refuse); one without data
    /// has nothing to confirm. Only a confirmable delete warns that it can't be undone.
    #[test]
    fn delete_data_confirm_is_off_when_installed_or_empty() {
        let undone = "This can\u{2019}t be undone.";
        let mut s = sample_state();
        s.apply_shot_hook("deletedata=Software/ritz");
        assert!(drawn_texts(&s).iter().any(|t| t == undone), "the confirmable delete warns");
        let mut s = sample_state();
        s.apply_shot_hook("deletedata=Software/SteamAlt");
        let texts = drawn_texts(&s);
        assert!(texts.iter().any(|t| t.contains("Remove it first, then delete its data.")), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("undone")), "no warning when Delete is off: {texts:?}");
        assert_eq!(press(&s, Some("Delete")), Vec::<Action>::new());
        let mut s = sample_state();
        s.apply_shot_hook("deletedata=Software/ritz");
        ritz_mut(&mut s).data.clear();
        assert_eq!(press(&s, Some("Close")), vec![Action::CloseModal]);
    }

    /// After a Remove that left data: "Delete data..." only opens the confirmation (the run is dismissed
    /// with it), Done only closes.
    #[test]
    fn remove_done_offers_delete_data_but_never_deletes() {
        let mut s = sample_state();
        test_render::finished_run(&mut s, "Software/ritz", Verb::Remove, RunPhase::Done { menu: false }, "ritz: removed");
        assert_eq!(press(&s, Some("Done")), vec![Action::CloseModal]);
        assert_eq!(
            press(&s, Some("Delete data\u{2026}")),
            vec![Action::CloseModal, Action::ShowModal(Modal::DeleteData { id: "Software/ritz".into() })]
        );
        // a program with no data left: no such button
        ritz_mut(&mut s).data.clear();
        assert!(std::panic::catch_unwind(|| press(&s, Some("Delete data\u{2026}"))).is_err());
    }

    #[test]
    fn delete_data_run_lines() {
        let mut s = sample_state();
        let subs = |s: &AppState, id: &str| sub_line(s.run.as_ref().unwrap(), s.listing.find(id)).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
        let done = RunPhase::Done { menu: false };
        // Done after a delete: no branch / version lines
        test_render::finished_run(&mut s, "Software/SteamAlt", Verb::DeleteData, done.clone(), "SteamAlt: data deleted");
        assert!(subs(&s, "Software/SteamAlt").is_empty());
        // Done after a remove: only the data left (the sample's ritz is not installed)
        test_render::finished_run(&mut s, "Software/ritz", Verb::Remove, done, "ritz: removed");
        assert_eq!(subs(&s, "Software/ritz"), ["Data left: 2 folders, 14 MB"]);
        // Failed delete: no "still on ..."
        test_render::finished_run(&mut s, "Software/SteamAlt", Verb::DeleteData, RunPhase::Failed { reason: "x".into() }, "x");
        assert!(subs(&s, "Software/SteamAlt").is_empty());
        // The fixture helper leaves no card bar of the install hook behind, and a finished delete has no data left
        assert!(s.progress.is_empty(), "no card progress: {:?}", s.progress);
        test_render::finished_run(&mut s, "Software/ritz", Verb::DeleteData, RunPhase::Done { menu: false }, "ritz: data deleted");
        assert!(s.progress.is_empty());
        assert!(s.listing.find("Software/ritz").unwrap().data.is_empty());
        assert!(subs(&s, "Software/ritz").is_empty(), "no \"Data left\" after the delete");
    }

    /// The dialog says "Deleting Ritz’s data…" / "Deleted Ritz’s data" / "Couldn’t delete Ritz’s data"; the
    /// other verbs keep their wording. A failure because the program is installed says what to do first.
    #[test]
    fn delete_data_run_titles_and_failed_note() {
        assert_eq!(run_title(Verb::DeleteData, Title::Running, "Ritz"), "Deleting Ritz\u{2019}s data\u{2026}");
        assert_eq!(run_title(Verb::DeleteData, Title::Done, "Ritz"), "Deleted Ritz\u{2019}s data");
        assert_eq!(run_title(Verb::DeleteData, Title::Failed, "Ritz"), "Couldn\u{2019}t delete Ritz\u{2019}s data");
        assert_eq!(run_title(Verb::Install, Title::Running, "Ritz"), "Installing Ritz\u{2026}");
        assert_eq!(run_title(Verb::Remove, Title::Done, "Ritz"), format!("{} Ritz", Verb::Remove.past()));
        assert_eq!(run_title(Verb::Update, Title::Failed, "Ritz"), "Couldn\u{2019}t update Ritz");

        assert_eq!(
            failed_note(Verb::DeleteData, "uninstall first: ritz-get remove Ritz", "Ritz"),
            "Nothing was deleted. Remove Ritz first, then delete its data."
        );
        assert_eq!(failed_note(Verb::DeleteData, "x", "Ritz"), "Nothing was deleted. Open the log for details, then try again.");
        assert_eq!(failed_note(Verb::Remove, "uninstall first", "Ritz"), "Open the log for details, then try again.");

        // and on screen
        let mut s = sample_state();
        let reason = "uninstall first: ritz-get remove SteamAlt".to_owned();
        test_render::finished_run(&mut s, "Software/SteamAlt", Verb::DeleteData, RunPhase::Failed { reason }, "x");
        let name = s.listing.find("Software/SteamAlt").unwrap().display().to_owned();
        let texts = drawn_texts(&s);
        assert!(texts.iter().any(|t| t == &format!("Couldn\u{2019}t delete {name}\u{2019}s data")), "{texts:?}");
        assert!(texts.iter().any(|t| t == &format!("Nothing was deleted. Remove {name} first, then delete its data.")), "{texts:?}");
    }

    /// A wrapped path continues one indent in; its first line (and a path that fits) does not move.
    #[test]
    fn wrapped_paths_hang() {
        let indent = "  ".repeat(HANG);
        assert_eq!(hang("~/.local/share/\nritz-get/\ngeofinder", &indent), "~/.local/share/\n  ritz-get/\n  geofinder");
        assert_eq!(hang("~/.config/ritz", &indent), "~/.config/ritz");
        // in the dialog: every continuation line of the (long) paths is indented, no first line is
        let mut s = sample_state();
        s.apply_shot_hook("deletedata=Software/ritz");
        ritz_mut(&mut s).data.push(crate::model::DataPath {
            path: "~/.local/share/ritz/some/deeply/nested/profile-folder/with/an/extremely/long/path/that/wraps/more/than/once/here".into(),
            kb: Some(1),
        });
        let texts = drawn_texts(&s);
        let long = texts.iter().find(|t| t.contains("deeply/")).expect("the long path is drawn");
        assert!(long.contains('\n'), "it wraps: {long:?}");
        for (i, l) in long.lines().enumerate() {
            assert_eq!(l.starts_with(&indent), i > 0, "line {i} of {long:?}");
        }
        assert_eq!(long.replace(['\n', ' '], ""), tilde("~/.local/share/ritz/some/deeply/nested/profile-folder/with/an/extremely/long/path/that/wraps/more/than/once/here"));
    }

    #[test]
    fn tilde_replaces_only_a_leading_home() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.len() < 2 {
            return;
        }
        assert_eq!(tilde(&format!("{home}/ritz-get/x")), "~/ritz-get/x");
        assert_eq!(tilde(&home), "~");
        assert_eq!(tilde(&format!("{home}er/x")), format!("{home}er/x"), "only whole components");
        assert_eq!(tilde("/opt/ritz-get"), "/opt/ritz-get");
    }

    /// A path wraps after a `/`, never inside `ritz-get`.
    #[test]
    fn long_paths_break_only_at_slashes() {
        let ctx = test_render::context();
        let font = theme::mono(11.5);
        let path = "~/.local/share/ritz-get/some-long-folder-name/geofinder";
        let mut shown = String::new();
        test_render::run(&ctx, 400.0, 200.0, |ui| shown = theme::wrap_at_slashes(ui, path, &font, 150.0));
        assert!(shown.contains('\n'), "{shown:?}");
        assert_eq!(shown.replace('\n', ""), path);
        for line in shown.lines().take(shown.lines().count() - 1) {
            assert!(line.ends_with('/'), "line {line:?} of {shown:?} does not end at a slash");
        }
    }

    /// Clicking the not-found banner's link pushes `OpenUrl` for the repo; Retry pushes `Retry`.
    #[test]
    fn banner_link_opens_the_repo() {
        let ctx = test_render::context();
        let state = AppState::default();
        let mut actions = Vec::new();
        let mut link = None;
        let mut click = Vec::new();
        for frame in 0..10 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 200.0))),
                events: std::mem::take(&mut click),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| draw_banner(ui, &state, &mut actions));
            });
            if frame == 5 {
                // Find the link text on screen, then press + release on it.
                for cs in &out.shapes {
                    if let egui::Shape::Text(t) = &cs.shape {
                        if t.galley.text() == "github.com/Ritze03/ritz-get" {
                            // Wrapped-layout labels carry the indent in their galley: take the text from its right end.
                            link = Some(t.galley.rect.translate(t.pos.to_vec2()).right_center() - egui::vec2(20.0, 0.0));
                        }
                    }
                }
                let pos = link.expect("the link is drawn");
                click = vec![egui::Event::PointerMoved(pos)];
            } else if frame == 6 || frame == 7 {
                let pressed = frame == 6;
                click = vec![egui::Event::PointerButton {
                    pos: link.unwrap(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }];
            }
        }
        assert_eq!(actions, vec![Action::OpenUrl("https://github.com/Ritze03/ritz-get".into())]);
    }
}
