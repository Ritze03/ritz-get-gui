//! Side panel for the selected card, plus the shared action list (**I10**).
//!
//! Mockup `renderSide` / `renderActions` (design/ritz-get-gui-graphite.html): hero (icon 88, name,
//! pills, description), then cards **Program / Additional steps required / Actions / Details**.
//! `ui::draw` already provides the PANEL2 frame (margin 8) and the scroll area; this module only
//! sizes its content to the width it is handed (nested-pane rule).
//!
//! * [`draw`]: the whole panel for `state.selected_entry()`.
//! * [`action_list`]: the three-group action list, shared with the cards' `...` popover (I9).
//! * [`auto_switch`]: the Auto-update switch widget (also usable by the popover).
//!
//! Panes never mutate state: everything is pushed into the `actions` list. Every command item is
//! disabled unless `state.actions_enabled()`; URL / folder / copy items stay enabled.

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, StrokeKind, vec2};

use crate::app::{AppState, CardProgress, Tone, Verb};
use crate::model::{Action, Badge, Entry, Modal, ShortcutState, Status};
use crate::theme::{self, glyph};
use crate::ui::pills::{self, show as show_pill, width as pill_width, Pill};

/// Draw the side panel content.
pub fn draw(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    let Some(e) = state.selected_entry() else {
        // Nothing to select at all (ritz-get missing / refused / first `list` pending): the groups
        // pane already says why, so the panel stays empty instead of promising actions.
        if state.listing.sections.is_empty() {
            return;
        }
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("Select a program to see its actions.").color(theme::DIM));
            ui.add_space(40.0);
        });
        return;
    };

    // The pane layout is top-down *justified*, which would justify (letter-space) wrapped labels:
    // draw everything in a plain top-down layout instead.
    ui.with_layout(Layout::top_down(Align::Min), |ui| body(ui, state, e, actions));
}

fn body(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    // Cards carry their own trailing GAP, so the vertical item spacing is zero.
    ui.spacing_mut().item_spacing.y = 0.0;

    hero(ui, state, e, actions);
    ui.add_space(theme::GAP);

    if e.is_modified() {
        modified_notice(ui, e);
        ui.add_space(theme::GAP);
    }
    if !e.is_website() && !e.is_unlisted() {
        theme::card(ui, "Program", |ui| program_card(ui, state, e, actions));
    }
    if matches!(state.badge(e), Some(Badge::SetupNeeded)) {
        theme::card(ui, "Setup needed", |ui| {
            let w = ui.available_width();
            rich_line(ui, "This program needs a one-time setup with admin rights. Run this in a terminal:", actions);
            let cmd = format!("ritz-get install {}", e.name);
            copy_field(ui, w, &cmd, actions);
        });
    }
    if !e.setup.is_empty() {
        theme::card(ui, "Additional steps required", |ui| {
            for (i, step) in e.setup.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // A fixed 14 px slot, so every step's text starts at the same x; the number sits
                    // on the first text line's baseline (the text rows are ~3 px taller than 12 px).
                    let (slot, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    ui.painter().text(
                        slot.left_top() + vec2(0.0, 3.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}.", i + 1),
                        theme::regular(12.0),
                        theme::DIM,
                    );
                    rich_line(ui, &step.text, actions);
                });
            }
        });
    }
    theme::card(ui, "Actions", |ui| action_list(ui, state, e, actions));
    theme::card(ui, "Details", |ui| details(ui, state, e));
}

// ---------------------------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------------------------

/// Centered, wrapping pill rows (greedy line packing, 4 px gaps) of the shared mapping.
fn pill_rows(ui: &mut egui::Ui, pills: &[Pill]) {
    const GAP: f32 = 4.0;
    let avail = ui.available_width();
    let widths: Vec<f32> = pills.iter().map(|p| pill_width(ui, p)).collect();
    let mut lines: Vec<std::ops::Range<usize>> = Vec::new();
    let (mut start, mut x) = (0, 0.0_f32);
    for (i, w) in widths.iter().enumerate() {
        let need = if i == start { *w } else { x + GAP + *w };
        if i > start && need > avail {
            lines.push(start..i);
            start = i;
            x = *w;
        } else {
            x = need;
        }
    }
    if start < pills.len() {
        lines.push(start..pills.len());
    }
    for (n, line) in lines.into_iter().enumerate() {
        if n > 0 {
            ui.add_space(GAP);
        }
        let total: f32 = widths[line.clone()].iter().sum::<f32>() + GAP * (line.len().saturating_sub(1)) as f32;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
            ui.spacing_mut().interact_size.y = theme::PILL_H;
            ui.horizontal(|ui| {
                ui.add_space(((avail - total) / 2.0).max(0.0));
                for p in &pills[line] {
                    show_pill(ui, p);
                }
            });
        });
    }
}

fn hero(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    let origin = ui.cursor().min;
    let avail = ui.available_width();
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        crate::icons::draw(ui, e, state.dir.as_deref(), 88.0);
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(e.display()).font(theme::bold(18.0)).color(theme::TEXT)).halign(Align::Center));
        ui.add_space(6.0);
        pill_rows(ui, &pills::pills(state, e));
        if !e.desc.is_empty() {
            ui.add_space(6.0);
            // mockup `.hero .desc`: max-width 34ch, 12 px, DIM
            let w = avail.min(240.0);
            ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(&e.desc).size(12.0).color(theme::DIM)).halign(Align::Center));
            });
        }
        if let Some(text) = data_left_line(e) {
            ui.add_space(6.0);
            let w = avail.min(240.0);
            ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(text).size(12.0).color(theme::DIM)).halign(Align::Center))
                    .on_hover_text(data_tooltip(e));
            });
        }
        ui.add_space(4.0);
    });

    // `↗` (mockup `.gh`, 20 px) in the top-right corner: the repo root (not for websites, D23).
    if let Some(url) = e.repo_url() {
        let rect = egui::Rect::from_min_size(egui::pos2(origin.x + avail - 20.0, origin.y), vec2(20.0, 20.0));
        let resp = ui.interact(rect, ui.id().with("side_gh"), Sense::click());
        let border = if resp.hovered() { theme::ACCENT } else { theme::BTNBD };
        let p = ui.painter();
        p.rect(rect, CornerRadius::same(5), theme::BTN, Stroke::new(1.0_f32, border), StrokeKind::Inside);
        let color = if resp.hovered() { theme::TEXT } else { theme::TEXT_DIM };
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "\u{2197}", theme::regular(12.0), color);
        super::groups::focus_ring(ui, &resp, rect, 5);
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open on GitHub");
        if resp.clicked() {
            actions.push(Action::OpenUrl(url));
        }
    }
}

/// "Data left: 2 folders, 14 MB": a program that is not installed but still has data on disk.
fn data_left_line(e: &Entry) -> Option<String> {
    if e.is_installed() || e.is_website() {
        return None;
    }
    e.data_label().map(|l| format!("Data left: {l}"))
}

/// Hover text of the data line / Data row: one `path (size)` per line.
fn data_tooltip(e: &Entry) -> String {
    let lines: Vec<String> = e
        .data
        .iter()
        .map(|d| match d.kb {
            Some(kb) => format!("{} ({})", d.path, crate::model::fmt_kb(kb)),
            None => d.path.clone(),
        })
        .collect();
    lines.join("\n")
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// D22: the modified notice. PORCELAIN only gives counts, not file names.
fn modified_notice(ui: &mut egui::Ui, e: &Entry) {
    let frame = egui::Frame::new()
        .fill(theme::with_alpha(theme::WARN, 0.16))
        .stroke(Stroke::new(1.0_f32, theme::with_alpha(theme::WARN, 0.42)))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::same(theme::CARD_PAD));
    theme::framed(ui, frame, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new("You changed files in this program, so updates are paused").size(12.0).color(theme::WARN));
        let mut local = Vec::new();
        if let Some(n) = e.dirty.filter(|n| *n > 0) {
            local.push(plural(n, "changed file", "changed files"));
        }
        if let Some(n) = e.ahead.filter(|n| *n > 0) {
            local.push(plural(n, "local commit", "local commits"));
        }
        if !local.is_empty() {
            ui.label(RichText::new(local.join(" \u{b7} ")).size(12.0).color(theme::TEXT));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Program card
// ---------------------------------------------------------------------------------------------

/// The Auto-update switch (mockup `.switch`: 34x18, accent when on, steel outline when off).
/// `on` = auto-update on (= not held). Returns the click response.
pub fn auto_switch(ui: &mut egui::Ui, on: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 18.0), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) }; // disabled = one step dimmer
        let (fill, border, knob) = if on {
            (theme::ACCENT, theme::ACCENT, theme::TEXT)
        } else {
            (theme::FIELD, theme::TEXT_DIM, theme::TEXT_DIM)
        };
        let p = ui.painter();
        let r = CornerRadius::same(9);
        p.rect(rect, r, fade(fill), Stroke::new(1.5_f32, fade(border)), StrokeKind::Inside);
        if enabled && resp.hovered() {
            p.rect_filled(rect.shrink(1.0), r, theme::HOV);
        }
        let x = rect.left() + 1.5 + 2.0 + 5.5 + if on { 16.0 } else { 0.0 };
        p.circle_filled(egui::pos2(x, rect.center().y), 5.5, fade(knob));
        super::groups::focus_ring(ui, &resp, rect, 9);
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// The caption under the Auto-update switch: the mockup's four variants.
fn auto_update_caption(e: &Entry) -> (String, Color32) {
    if !e.is_installed() {
        ("available once installed".into(), theme::TEXT_FAINT)
    } else if e.is_modified() {
        ("modified \u{b7} updates skipped until you reset".into(), theme::WARN)
    } else if e.held {
        (format!("held \u{b7} ritz-get unhold {}", e.name), theme::TEXT_FAINT)
    } else {
        ("included in Update all".into(), theme::TEXT_FAINT)
    }
}

fn program_card(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    let cmd = state.actions_enabled();
    theme::columns(ui, 2, |c| {
        theme::row_label(&mut c[0], "Auto-update");
        let w = c[1].available_width();
        c[1].allocate_ui_with_layout(vec2(w, theme::CTRL_H), Layout::right_to_left(Align::Center), |ui| {
            if auto_switch(ui, e.is_installed() && !e.held, e.is_installed() && cmd).clicked() {
                actions.push(if e.held { Action::Unhold { name: e.name.clone() } } else { Action::Hold { name: e.name.clone() } });
            }
        });
    });
    let (text, color) = auto_update_caption(e);
    ui.label(RichText::new(text).size(11.0).color(color));

    theme::columns(ui, 2, |c| {
        theme::row_label(&mut c[0], "Branch");
        branch_combo(&mut c[1], state, e, actions);
    });
}

/// D2: the branch ComboBox. Options come from `state.branches[name]`; when they are not known yet
/// we show the current branch and ask for them once, when the box opens.
fn branch_combo(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    // The picked-but-not-applied branch shows as selected; "* b" still marks the checked-out one.
    let cur = e.pending_branch.clone().or_else(|| e.branch.clone()).unwrap_or_default();
    let known = state.branches.get(&e.name);
    let asked = egui::Id::new(("side_branches_asked", &e.name));
    {
        // FIELD like the mockup's `select.input`
        let w = &mut ui.visuals_mut().widgets;
        for v in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            v.bg_fill = theme::FIELD;
            v.weak_bg_fill = theme::FIELD;
        }
    }
    ui.add_enabled_ui(state.actions_enabled(), |ui| {
        let shown = if cur.is_empty() { "\u{2014}".to_owned() } else { cur.clone() };
        let r = egui::ComboBox::from_id_salt(("side_branch", &e.name))
            .width(ui.available_width())
            .icon(theme::combo_chevron)
            .selected_text(RichText::new(shown).size(12.0))
            .show_ui(ui, |ui| match known {
                None => {
                    ui.add_enabled(false, egui::Label::new(RichText::new("Loading\u{2026}").size(12.0)));
                    let first = ui.ctx().data_mut(|d| d.get_temp::<bool>(asked).unwrap_or(false)) == false;
                    if first {
                        ui.ctx().data_mut(|d| d.insert_temp(asked, true));
                        actions.push(Action::Branches { name: e.name.clone() });
                    }
                }
                Some(list) => {
                    for (b, is_current) in list {
                        let label = if *is_current { format!("* {b}") } else { b.clone() };
                        let picked = ui.selectable_label(*b == cur, RichText::new(label).size(12.0)).clicked();
                        if picked && *b != cur {
                            actions.push(Action::PickBranch { name: e.name.clone(), branch: b.clone() });
                        }
                    }
                }
            });
        if r.inner.is_none() {
            ui.ctx().data_mut(|d| d.remove_temp::<bool>(asked)); // closed: ask again next time
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Setup steps (rich text), copy field
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Span<'a> {
    Text(&'a str),
    Code(&'a str),
    Url(&'a str),
}

/// Split a setup line into plain text, `code` spans and http(s) URLs (mockup `richLine`).
fn spans(text: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let tick = rest.find('`');
        let url = [rest.find("http://"), rest.find("https://")].into_iter().flatten().min();
        match (tick, url) {
            (None, None) => {
                out.push(Span::Text(rest));
                break;
            }
            (Some(t), u) if u.is_none_or(|u| t < u) => {
                if t > 0 {
                    out.push(Span::Text(&rest[..t]));
                }
                let after = &rest[t + 1..];
                match after.find('`') {
                    Some(j) => {
                        out.push(Span::Code(&after[..j]));
                        rest = &after[j + 1..];
                    }
                    None => {
                        out.push(Span::Text(&rest[t..]));
                        break;
                    }
                }
            }
            (_, Some(u)) => {
                if u > 0 {
                    out.push(Span::Text(&rest[..u]));
                }
                let tail = &rest[u..];
                let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
                let url = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '\'', '"']);
                out.push(Span::Url(url));
                rest = &tail[url.len()..];
            }
            (Some(_), None) => unreachable!("covered by the tick arm"),
        }
    }
    out
}

/// One wrapping line of rich text: plain DIM-free TEXT, clickable URLs (ACCENT, underlined) and
/// code spans in Geist Mono on FIELD.
fn rich_line(ui: &mut egui::Ui, text: &str, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
        ui.spacing_mut().interact_size.y = 14.0;
        for s in spans(text) {
            match s {
                Span::Text(t) => {
                    ui.label(RichText::new(t).size(12.0).color(theme::TEXT));
                }
                Span::Code(c) => code_chip(ui, c),
                Span::Url(u) => {
                    let r = ui
                        .add(egui::Label::new(RichText::new(u).size(12.0).color(theme::ACCENT).underline()).sense(Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        actions.push(Action::OpenUrl(u.to_owned()));
                    }
                }
            }
        }
    });
}

/// `.setup-list code`: one non-wrapping unit (it wraps to the next line as a whole), Geist Mono
/// 11.5 on FIELD with a 1 px BORDER, radius 5 and 4 px horizontal padding. No outer margin, so a
/// following period or comma hugs the chip.
fn code_chip(ui: &mut egui::Ui, code: &str) {
    let g = ui.painter().layout_no_wrap(code.to_owned(), theme::mono(11.5), theme::TEXT);
    let size = vec2(g.size().x + 2.0 * (4.0 + 1.0), g.size().y + 2.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(rect, CornerRadius::same(5), theme::FIELD, Stroke::new(1.0_f32, theme::BORDER), StrokeKind::Inside);
        p.galley(rect.min + vec2(5.0, 1.0), g, theme::TEXT);
    }
}

/// A selectable Geist Mono field with a Copy button (D48 exit-4 hint, mockup `.input`).
fn copy_field(ui: &mut egui::Ui, width: f32, text: &str, actions: &mut Vec<Action>) {
    let gap = ui.spacing().item_spacing.x;
    // Wide enough for "Copied" too, so the flash does not resize the button.
    let btn_w = ["Copy", "Copied"]
        .iter()
        .map(|t| ui.painter().layout_no_wrap((*t).into(), theme::regular(13.0), theme::TEXT).size().x)
        .fold(0.0_f32, f32::max)
        .ceil()
        + 18.0
        + 2.0;
    ui.add_space(theme::ROW_GAP);
    theme::columns_sized(ui, &[(width - btn_w - gap).max(0.0), btn_w], gap, |c| {
        let frame = egui::Frame::new()
            .fill(theme::FIELD)
            .stroke(Stroke::new(1.0_f32, theme::BTNBD))
            .corner_radius(theme::RADIUS)
            .inner_margin(Margin::symmetric(6, 3));
        theme::framed(&mut c[0], frame, |ui| {
            // Plain top-down: the pane's justified layout would letter-space a wrapped line.
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                // One line: elided (full text on hover) only when it truly doesn't fit.
                let r = ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.5)).color(theme::TEXT)).selectable(true).truncate());
                r.on_hover_text(text);
            });
        });
        let key = ("side_copy_field", text);
        let copied = theme::flashing(c[1].ctx(), key);
        if c[1].add_sized([btn_w, theme::CTRL_H], theme::secondary_button(if copied { "Copied" } else { "Copy" })).clicked() {
            actions.push(Action::CopyToClipboard(text.to_owned()));
            theme::flash(c[1].ctx(), key);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Details card
// ---------------------------------------------------------------------------------------------

/// The Details rows `(label, value)`; values are drawn in Geist Mono. The website `url` row shows
/// the full URL without its scheme; [`detail_tooltip`] gives the complete URL for its hover.
fn detail_rows(state: &AppState, e: &Entry) -> Vec<(&'static str, String)> {
    let mut rows = vec![("id", e.name.clone())];
    if let Some(url) = e.website_url() {
        rows.push(("url", super::groups::strip_scheme(url).to_owned()));
        return rows;
    }
    if let Some(src) = &e.source {
        rows.push(("source", src.clone()));
    }
    if let Some(b) = &e.branch {
        rows.push(("branch", b.clone()));
    }
    if let (true, Some(dir)) = (e.is_installed(), &state.dir) {
        rows.push(("path", dir.join(&e.name).to_string_lossy().into_owned()));
    }
    rows.push(("installed", e.installed.as_ref().map_or_else(|| "no".to_owned(), |v| v.to_string())));
    if let Some(l) = &e.latest {
        rows.push(("latest", l.to_string()));
    }
    if let Some(n) = e.behind.filter(|n| *n > 0) {
        rows.push(("behind", plural(n, "commit", "commits")));
    }
    if let Some(n) = e.ahead.filter(|n| *n > 0) {
        rows.push(("ahead", plural(n, "commit", "commits")));
    }
    if let Some(Badge::Needs(tools)) = state.badge(e) {
        rows.push(("missing tool", tools.join(", ")));
    }
    if let Some(n) = e.dirty.filter(|n| *n > 0) {
        rows.push(("local", plural(n, "changed file", "changed files")));
    }
    // Not installed: the hero's "Data left" line already says it.
    if let (true, Some(l)) = (e.is_installed(), e.data_label()) {
        rows.push(("Data", l));
    }
    rows
}

/// Hover text of a Details row: the full URL behind the website `url` row, the data paths behind
/// the `Data` row.
fn detail_tooltip(e: &Entry, label: &str) -> Option<String> {
    match label {
        "url" => e.website_url().map(str::to_owned),
        "Data" => Some(data_tooltip(e)),
        _ => None,
    }
}

fn details(ui: &mut egui::Ui, state: &AppState, e: &Entry) {
    let rows = detail_rows(state, e);
    // 62 px, widened for the longest label ("missing tool").
    let label_w = rows
        .iter()
        .map(|(l, _)| ui.painter().layout_no_wrap((*l).to_owned(), theme::regular(12.0), theme::DIM).size().x.ceil() + 4.0)
        .fold(62.0_f32, f32::max);
    for (label, value) in rows {
        let tip = detail_tooltip(e, label);
        let gap = ui.spacing().item_spacing.x;
        let w = ui.available_width();
        theme::columns_sized(ui, &[label_w, (w - label_w - gap).max(0.0)], gap, |c| {
            let h = theme::CTRL_H;
            c[0].allocate_ui_with_layout(vec2(label_w, h), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(label).size(12.0).color(theme::DIM));
            });
            let vw = c[1].available_width();
            // Long paths / URLs break after a `/`, never at a hyphen inside a name.
            let value = theme::wrap_at_slashes(&c[1], &value, &theme::mono(12.0), vw);
            let galley = c[1].painter().layout(value.clone(), theme::mono(12.0), theme::TEXT_DIM, vw);
            c[1].add_space(((h - galley.size().y) / 2.0).max(0.0));
            c[1].with_layout(Layout::top_down(Align::Max), |ui| {
                let r = ui.add(egui::Label::new(RichText::new(value).font(theme::mono(12.0)).color(theme::TEXT_DIM)).selectable(true));
                if let Some(t) = tip {
                    r.on_hover_text(t);
                }
                ui.add_space(((h - galley.size().y) / 2.0).max(0.0));
            });
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Action list
// ---------------------------------------------------------------------------------------------

/// One row of the action list.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    glyph: &'static str,
    text: String,
    danger: bool,
    enabled: bool,
    /// `None` = informative, never clickable ("Up to date").
    action: Option<Action>,
    /// A progress row (running / finished install, update, ...): coloured by its tone, never clickable.
    tone: Option<Tone>,
}

/// A ritz-get command (or the Reset dialog): disabled while busy / refused / ritz-get missing.
fn cmd_item(state: &AppState, glyph: &'static str, text: &str, action: Action) -> Item {
    Item { glyph, text: text.to_owned(), danger: false, enabled: state.actions_enabled(), action: Some(action), tone: None }
}

/// URL / folder / clipboard: always enabled.
fn free_item(glyph: &'static str, text: &str, action: Action) -> Item {
    Item { glyph, text: text.to_owned(), danger: false, enabled: true, action: Some(action), tone: None }
}

fn danger(mut i: Item) -> Item {
    i.danger = true;
    i
}

/// The three groups of the action list (lifecycle · local · GitHub), empty groups dropped.
fn groups(state: &AppState, e: &Entry) -> Vec<Vec<Item>> {
    let name = || e.name.clone();
    let shortcut = match e.shortcut {
        ShortcutState::On => Some(cmd_item(state, glyph::MINUS, "Remove shortcut", Action::Shortcut { name: name(), remove: true })),
        ShortcutState::Off => Some(cmd_item(state, glyph::PLUS, "Create shortcut", Action::Shortcut { name: name(), remove: false })),
        ShortcutState::None => None, // D60: no desktop file, no shortcut action
    };
    let remove = || danger(cmd_item(state, glyph::TRASH, "Remove", Action::Remove { name: name() }));

    let groups = if e.is_website() {
        vec![vec![cmd_item(state, glyph::EXT, "Open", Action::Open { name: name() })], shortcut.into_iter().collect()]
    } else if e.is_unlisted() {
        vec![vec![remove()]] // D54
    } else if !e.is_installed() {
        // Mockup `actionGroups`: the local group stays, with its rows disabled until installed.
        let off = |glyph, text: &str| Item { glyph, text: text.to_owned(), danger: false, enabled: false, action: None, tone: None };
        let mut local: Vec<Item> = shortcut.map(|i| Item { enabled: false, action: None, ..i }).into_iter().collect();
        local.push(off(glyph::FOLDER, "Open folder"));
        let mut gh = Vec::new();
        github_items(e, &mut gh);
        // Leftover data of a removed program (an installed one must be removed first).
        let data: Vec<Item> = if e.data.is_empty() {
            Vec::new()
        } else {
            vec![danger(cmd_item(state, glyph::TRASH, "Delete data\u{2026}", Action::ShowModal(Modal::DeleteData { id: e.id() })))]
        };
        vec![vec![cmd_item(state, glyph::DOWNLOAD, "Install", e.install_action())], local, gh, data]
    } else {
        let mut life = Vec::new();
        if e.has_update() {
            let mut u = cmd_item(state, glyph::REFRESH, "Update", e.update_action());
            u.enabled &= !e.is_modified(); // update refuses a modified clone
            life.push(u);
        } else if e.status == Status::Current {
            life.push(Item { glyph: glyph::CHECK, text: "Up to date".into(), danger: false, enabled: false, action: None, tone: None });
        }
        life.push(cmd_item(state, glyph::GEAR, "Repair", Action::Repair { name: name() }));
        life.push(remove());
        if e.is_modified() {
            life.push(danger(cmd_item(
                state,
                glyph::UNDO,
                "Reset (discard local changes)",
                Action::ShowModal(Modal::Reset { id: e.id() }),
            )));
        }
        let mut local: Vec<Item> = shortcut.into_iter().collect();
        if let Some(dir) = &state.dir {
            local.push(free_item(glyph::FOLDER, "Open folder", Action::OpenFolder(dir.join(&e.name).to_string_lossy().into_owned())));
        }
        let mut gh = Vec::new();
        github_items(e, &mut gh);
        vec![life, local, gh]
    };
    let mut groups: Vec<Vec<Item>> = groups.into_iter().filter(|g| !g.is_empty()).collect();
    if let (Some(cp), Some(life)) = (state.progress.get(&e.name), groups.first_mut()) {
        progress_row(cp, life);
    }
    groups
}

/// The running / just finished command's row: replaces its lifecycle row (Install / Update / "Up to
/// date" are the first row; Repair its own) with the bar's label, small and not clickable. Switch
/// and Reset have no row of their own.
fn progress_row(cp: &CardProgress, life: &mut Vec<Item>) {
    let tone = cp.tone();
    let row = Item {
        glyph: match tone {
            Tone::Good | Tone::Even => glyph::CHECK,
            Tone::Danger => glyph::WARNING,
            Tone::Warn => glyph::PAUSE,
            _ => glyph::REFRESH,
        },
        text: cp.label().to_owned(),
        danger: false,
        enabled: false,
        action: None,
        tone: Some(tone),
    };
    match cp.verb {
        Verb::Repair => {
            if let Some(i) = life.iter().position(|it| it.text == "Repair") {
                life[i] = row;
            }
        }
        Verb::Install | Verb::Update => {
            if life.first().is_some_and(|it| matches!(it.text.as_str(), "Install" | "Update" | "Up to date")) {
                life[0] = row;
            } else {
                life.insert(0, row);
            }
        }
        Verb::Switch | Verb::Reset | Verb::Remove | Verb::DeleteData => {}
    }
}

/// D23: Open on GitHub / Open README / Report issue (programs with a repo only).
fn github_items(e: &Entry, out: &mut Vec<Item>) {
    let Some(repo) = e.repo_url() else { return };
    let branch = e.branch.as_deref().unwrap_or("HEAD");
    out.push(free_item(glyph::GITHUB, "Open on GitHub", Action::OpenUrl(repo.clone())));
    out.push(free_item(glyph::BOOK, "Open README", Action::OpenUrl(format!("{repo}/tree/{branch}#readme"))));
    out.push(free_item(glyph::BUG, "Report issue", Action::OpenUrl(format!("{repo}/issues/new"))));
}

/// Paint one action row (mockup `.action`: min 26 px, 6 px side padding, 16 px glyph slot, 8 px gap,
/// HOV wash on hover, text wraps). Returns true when clicked.
fn action_row(ui: &mut egui::Ui, width: f32, it: &Item) -> bool {
    let (text_col, glyph_col) = if let Some(tone) = it.tone {
        let c = super::groups::tone_colors(tone).2;
        (c, c)
    } else if !it.enabled {
        (theme::FAINT, theme::FAINT)
    } else if it.danger {
        (theme::DANGER, theme::DANGER)
    } else {
        (theme::TEXT, theme::TEXT_DIM)
    };
    let text_w = (width - 6.0 - 16.0 - 8.0 - 6.0).max(24.0);
    let galley = ui.painter().layout(it.text.clone(), theme::regular(13.0), text_col, text_w);
    let h = (galley.size().y + 4.0).max(26.0);
    let sense = if it.enabled && it.action.is_some() { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), sense);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if resp.hovered() && sense == Sense::click() {
            p.rect_filled(rect, CornerRadius::same(theme::RADIUS), theme::HOV);
        }
        theme::paint_glyph(p, egui::pos2(rect.left() + 6.0 + 8.0, rect.center().y), it.glyph, 13.0, glyph_col);
        p.galley(egui::pos2(rect.left() + 6.0 + 16.0 + 8.0, rect.center().y - galley.size().y / 2.0), galley, text_col);
        super::groups::focus_ring(ui, &resp, rect, theme::RADIUS);
        if it.tone == Some(Tone::Busy) {
            // A thin indeterminate sweep along the row's bottom edge.
            let track = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 6.0, rect.bottom() - 4.0),
                egui::pos2(rect.right() - 6.0, rect.bottom() - 2.0),
            );
            let t = ui.ctx().input(|i| i.time);
            p.rect_filled(track, 1.0, theme::BORDER);
            p.rect_filled(super::groups::sweep_rect(track, t), 1.0, theme::ACCENT);
        }
    }
    let resp = if sense == Sense::click() { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    resp.clicked()
}

/// The action list for `entry` (Install / Update / Repair / Remove / Reset / shortcut / Open folder /
/// GitHub items), shared by the side panel's Actions card and the cards' `...` popover (I9).
/// Three groups with a separator line between them; rows fill the width the `ui` offers, so a
/// popover must give it a fixed width first (`ui.set_width(~240.0)`).
pub fn action_list(ui: &mut egui::Ui, state: &AppState, entry: &Entry, actions: &mut Vec<Action>) {
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (n, group) in groups(state, entry).iter().enumerate() {
            if n > 0 {
                ui.add_space(3.0);
                let (r, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, theme::BORDER);
                ui.add_space(3.0);
            }
            for it in group {
                if action_row(ui, width, it) {
                    if let Some(a) = &it.action {
                        actions.push(a.clone());
                    }
                }
            }
        }
    });
}

/// Every action [`action_list`] offers for `entry` (clickable or not), in order. The pure helper
/// behind the whole-app call-log tests ("no shortcut action for `-`").
#[cfg(test)]
pub fn offered_actions(state: &AppState, entry: &Entry) -> Vec<Action> {
    groups(state, entry).into_iter().flatten().filter_map(|i| i.action).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> AppState {
        let mut s = AppState::sample();
        s.dir = Some("/home/u/src".into());
        s
    }

    fn texts(g: &[Vec<Item>]) -> Vec<Vec<&str>> {
        g.iter().map(|g| g.iter().map(|i| i.text.as_str()).collect()).collect()
    }

    fn find<'a>(s: &'a AppState, id: &str) -> &'a Entry {
        s.listing.find(id).unwrap_or_else(|| panic!("sample has no {id}"))
    }

    /// A short command stays on one line in the 300 px side panel (no "install / geofinder" wrap).
    #[test]
    fn copy_field_keeps_a_short_command_on_one_line() {
        let ctx = crate::ui::test_render::context();
        let mut h = 0.0;
        let (out, _) = crate::ui::test_render::run(&ctx, 300.0, 200.0, |ui| {
            let mut actions = Vec::new();
            let r = ui.scope(|ui| copy_field(ui, 264.0, "ritz-get install geofinder", &mut actions)).response.rect;
            h = r.height();
        });
        assert!(h <= theme::ROW_GAP + 26.0, "copy field is {h} px tall (wrapped?)");
        let elided = out.shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.elided));
        assert!(!elided, "the command was elided although it fits");
    }

    #[test]
    fn spans_split_text_code_and_urls() {
        assert_eq!(
            spans("Set `ritz %command%`. See https://a.b/c, ok"),
            vec![
                Span::Text("Set "),
                Span::Code("ritz %command%"),
                Span::Text(". See "),
                Span::Url("https://a.b/c"),
                Span::Text(", ok"),
            ]
        );
        assert_eq!(spans("plain"), vec![Span::Text("plain")]);
        assert_eq!(spans("open (http://x.y/z)"), vec![Span::Text("open ("), Span::Url("http://x.y/z"), Span::Text(")")]);
        assert_eq!(spans("broken `tick"), vec![Span::Text("broken "), Span::Text("`tick")]);
    }

    #[test]
    fn installed_behind_has_all_three_groups() {
        let s = st();
        let e = find(&s, "Helpers/ts6tray");
        let g = groups(&s, e);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["Update", "Repair", "Remove"]);
        assert!(g[2].iter().all(|i| i.enabled), "URL items are always enabled");
        assert!(g[1].iter().any(|i| i.text == "Open folder"));
    }

    #[test]
    fn shortcut_item_follows_column_17() {
        let s = st();
        let label = |id: &str| {
            groups(&s, find(&s, id)).into_iter().flatten().find(|i| i.text.contains("shortcut")).map(|i| (i.text, i.action))
        };
        let (t, a) = label("Software/yt-dlp-gui").expect("on -> Remove shortcut"); // shortcut=on
        assert_eq!(t, "Remove shortcut");
        assert_eq!(a, Some(Action::Shortcut { name: "yt-dlp-gui".into(), remove: true }));
        let (t, a) = label("Helpers/ccorral").expect("off -> Create shortcut"); // shortcut=off
        assert_eq!(t, "Create shortcut");
        assert_eq!(a, Some(Action::Shortcut { name: "ccorral".into(), remove: false }));
        assert!(label("Helpers/ts6tray").is_none(), "shortcut=- hides the item");
    }

    #[test]
    fn modified_gets_reset_and_update_disabled() {
        let s = st();
        let e = find(&s, "Software/geofinder");
        assert!(e.is_modified());
        let g = groups(&s, e);
        let reset = g[0].iter().find(|i| i.text.starts_with("Reset")).expect("reset item");
        assert_eq!(reset.action, Some(Action::ShowModal(Modal::Reset { id: "Software/geofinder".into() })));
        if let Some(u) = g[0].iter().find(|i| i.text == "Update") {
            assert!(!u.enabled);
        }
    }

    #[test]
    fn websites_and_unlisted_and_not_installed() {
        let s = st();
        let web = find(&s, "Websites/cs2-alias-cycling");
        let g = groups(&s, web);
        assert_eq!(g[0].len(), 1);
        assert_eq!(g[0][0].action, Some(Action::Open { name: "cs2-alias-cycling".into() }));
        assert!(g.iter().flatten().all(|i| !matches!(i.action, Some(Action::OpenUrl(_)))), "no URL items for websites");

        let unl = find(&s, "Unlisted/old-experiment");
        assert_eq!(texts(&groups(&s, unl)), vec![vec!["Remove"]]);

        let missing = find(&s, "Software/ForzaTelemetryV3Launcher");
        let g = groups(&s, missing);
        assert_eq!(g[0][0].action, Some(Action::Install { name: "ForzaTelemetryV3Launcher".into(), branch: None }));
        assert!(!g.iter().flatten().any(|i| i.text == "Remove"));
        // The local group stays, with a disabled Open folder row; no shortcut row for `-` (D60).
        assert_eq!(texts(&g), vec![vec!["Install"], vec!["Open folder"], vec!["Open on GitHub", "Open README", "Report issue"]]);
        let folder = &g[1][0];
        assert!(!folder.enabled && folder.action.is_none());
    }

    #[test]
    fn website_url_row_is_the_full_url_without_scheme_with_the_full_url_as_tooltip() {
        let s = st();
        let e = find(&s, "Websites/cs2-multimap");
        let rows = detail_rows(&s, e);
        let url = rows.iter().find(|(l, _)| *l == "url").expect("url row");
        assert_eq!(url.1, "ritze03.github.io/stupid-pages/cs2-multimap-generator.html");
        assert_eq!(detail_tooltip(e, "url").as_deref(), e.website_url());
        assert!(detail_tooltip(e, "id").is_none());
    }

    #[test]
    fn needs_badge_adds_a_missing_tool_row_before_local() {
        let mut s = st();
        s.badges.insert("geofinder".into(), Badge::Needs(vec!["cargo".into(), "pkg-config".into()]));
        let rows = detail_rows(&s, find(&s, "Software/geofinder"));
        let labels: Vec<_> = rows.iter().map(|r| r.0).collect();
        let i = labels.iter().position(|l| *l == "missing tool").expect("missing tool row");
        assert_eq!(rows[i].1, "cargo, pkg-config");
        assert_eq!(labels.get(i + 1), Some(&"local"));
        let s = st();
        assert!(!detail_rows(&s, find(&s, "Software/geofinder")).iter().any(|r| r.0 == "missing tool"));
    }

    #[test]
    fn command_items_disabled_while_busy() {
        let mut s = st();
        s.busy = true;
        let e = find(&s, "Helpers/ts6tray");
        for it in groups(&s, e).into_iter().flatten() {
            let is_cmd = matches!(
                it.action,
                Some(Action::Update { .. } | Action::Repair { .. } | Action::Remove { .. } | Action::Shortcut { .. })
            );
            assert_eq!(it.enabled, !is_cmd, "{}", it.text);
        }
    }

    #[test]
    fn delete_data_is_offered_only_for_a_not_installed_program_with_data() {
        let s = st();
        let del = |s: &AppState, id: &str| groups(s, find(s, id)).into_iter().flatten().find(|i| i.text == "Delete data\u{2026}");
        let modal = Action::ShowModal(Modal::DeleteData { id: "Software/ritz".into() });
        let it = del(&s, "Software/ritz").expect("ritz: not installed, has data");
        assert!(it.danger && it.enabled);
        assert_eq!(it.glyph, glyph::TRASH);
        assert_eq!(it.action, Some(modal.clone()));
        assert!(offered_actions(&s, find(&s, "Software/ritz")).contains(&modal));
        // its own last group, after Install / local / GitHub
        let g = groups(&s, find(&s, "Software/ritz"));
        assert_eq!(texts(&g[g.len() - 1..]), vec![vec!["Delete data\u{2026}"]]);
        assert!(del(&s, "Software/SteamAlt").is_none(), "installed: remove it first");
        assert!(del(&s, "Websites/cs2-alias-cycling").is_none());
        assert!(del(&s, "Software/ForzaTelemetryV3Launcher").is_none(), "no data");
        assert!(del(&s, "Unlisted/old-experiment").is_none());
        // disabled like the other commands while one runs
        let mut busy = st();
        busy.busy = true;
        assert!(!del(&busy, "Software/ritz").unwrap().enabled);
    }

    #[test]
    fn data_row_for_installed_programs_and_data_left_line_for_missing_ones() {
        let s = st();
        let rows = detail_rows(&s, find(&s, "Software/SteamAlt"));
        assert!(rows.contains(&("Data", "1 folder, 340 KB".to_owned())), "{rows:?}");
        assert!(!detail_rows(&s, find(&s, "Software/ritz")).iter().any(|r| r.0 == "Data"), "the hero says it");
        assert!(!detail_rows(&s, find(&s, "Helpers/ts6tray")).iter().any(|r| r.0 == "Data"));
        assert_eq!(data_left_line(find(&s, "Software/ritz")).as_deref(), Some("Data left: 2 folders, 14 MB"));
        assert_eq!(data_left_line(find(&s, "Software/SteamAlt")), None);
        assert_eq!(data_tooltip(find(&s, "Software/ritz")), "~/.config/ritz (12 MB)\n~/.local/share/ritz (2 MB)");
    }

    #[test]
    fn side_panel_paints_the_data_left_line_at_every_width() {
        let s = st();
        let e = find(&s, "Software/ritz");
        for w in [300.0_f32, 240.0] {
            let ctx = crate::ui::test_render::context();
            let (out, _) = crate::ui::test_render::run(&ctx, w, 900.0, |ui| {
                let mut actions = Vec::new();
                body(ui, &s, e, &mut actions);
            });
            let text: Vec<String> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            assert!(text.iter().any(|t| t == "Data left: 2 folders, 14 MB"), "{w}px: {text:?}");
            assert!(text.iter().any(|t| t == "Delete data\u{2026}"), "{w}px: {text:?}");
        }
    }

    #[test]
    fn readme_url_uses_branch() {
        let s = st();
        let e = find(&s, "Helpers/ts6tray");
        let urls: Vec<_> = groups(&s, e)
            .into_iter()
            .flatten()
            .filter_map(|i| match i.action {
                Some(Action::OpenUrl(u)) => Some(u),
                _ => None,
            })
            .collect();
        assert_eq!(
            urls,
            [
                "https://github.com/Ritze03/ts6tray",
                "https://github.com/Ritze03/ts6tray/tree/main#readme",
                "https://github.com/Ritze03/ts6tray/issues/new"
            ]
        );
    }

    #[test]
    fn caption_variants() {
        let s = st();
        let cap = |id: &str| auto_update_caption(find(&s, id)).0;
        assert_eq!(cap("Software/ForzaTelemetryV3Launcher"), "available once installed");
        assert_eq!(cap("Software/geofinder"), "modified \u{b7} updates skipped until you reset");
        assert_eq!(cap("Software/yt-dlp-gui"), "held \u{b7} ritz-get unhold yt-dlp-gui");
        assert_eq!(cap("Helpers/ts6tray"), "included in Update all");
    }
}
