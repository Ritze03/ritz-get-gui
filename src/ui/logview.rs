//! The scrolling build-log view shared by the progress dialog (its "Show log" expander) and the
//! log dialog (status bar "Log"). It replaced the bottom log drawer.
//!
//! A FIELD box (1 px BORDER, radius 7) of a fixed height with Geist Mono lines coloured by kind
//! (Info TEXT, Cmd TEXT_DIM, Ok GOOD, Err DANGER, Note ACCENT), an optional faint `HH:MM:SS`
//! column, long lines wrapped inside the box. Only the visible lines are laid out (cumulative
//! wrapped heights are cached in egui memory and recomputed when the width changes or the cap drops
//! the head), and the view sticks to the bottom while new lines arrive.

use egui::{pos2, vec2, Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, TextWrapMode, UiBuilder};

use crate::model::{LogKind, LogLine};
use crate::theme;

const FONT_PT: f32 = 11.5;
const LINE_H: f32 = FONT_PT * 1.5;
/// Space between the time column and the text.
const TIME_GAP: f32 = 12.0;
const PAD: egui::Vec2 = vec2(10.0, 8.0);

fn kind_color(kind: LogKind) -> Color32 {
    match kind {
        LogKind::Info => theme::TEXT,
        LogKind::Cmd => theme::TEXT_DIM,
        LogKind::Ok => theme::GOOD,
        LogKind::Err => theme::DANGER,
        LogKind::Note => theme::ACCENT,
    }
}

fn line_text(l: &LogLine) -> RichText {
    RichText::new(&l.text).font(theme::mono(FONT_PT)).line_height(Some(LINE_H)).color(kind_color(l.kind))
}

/// Cumulative bottom edge of every log line at one text width (`ends[i]` = y after line `i`).
#[derive(Clone, Default)]
struct LineHeights {
    width: f32,
    /// The line at index 0 when computed; if it changes, the cap dropped the head.
    first: Option<(String, String)>,
    ends: Vec<f32>,
}

impl LineHeights {
    fn refresh(&mut self, ui: &egui::Ui, log: &[LogLine], text_w: f32) {
        let first = log.first().map(|l| (l.time.clone(), l.text.clone()));
        if (self.width - text_w).abs() > 0.5 || self.first != first || self.ends.len() > log.len() {
            *self = LineHeights { width: text_w, first, ends: Vec::with_capacity(log.len()) };
        }
        let mut y = self.ends.last().copied().unwrap_or(0.0);
        for l in &log[self.ends.len()..] {
            let g = egui::WidgetText::from(line_text(l)).into_galley(ui, Some(TextWrapMode::Wrap), text_w, egui::TextStyle::Monospace);
            y += g.size().y.max(LINE_H);
            self.ends.push(y);
        }
    }
}

/// How many of the newest lines the progress dialog's log shows.
pub const RUN_LINES: usize = 200;

/// The log box, `w` x `h`. `salt` keeps the scroll position and the height cache apart from other
/// log views; `times` adds the `HH:MM:SS` column.
pub fn show(ui: &mut egui::Ui, salt: &str, lines: &[LogLine], w: f32, h: f32, times: bool, touch: bool) {
    let (body, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if !ui.is_rect_visible(body) {
        return;
    }
    ui.painter().rect(body, CornerRadius::same(theme::RADIUS), theme::FIELD, Stroke::new(1.0_f32, theme::BORDER), egui::StrokeKind::Inside);
    let inner = body.shrink2(PAD);
    let mut bui = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::LEFT)));
    bui.set_clip_rect(body.shrink(1.0).intersect(ui.clip_rect()));

    if lines.is_empty() {
        bui.add(egui::Label::new(RichText::new("Command output appears here.").font(theme::mono(FONT_PT)).color(theme::TEXT_FAINT)));
        return;
    }

    let time_w = if times { bui.painter().layout_no_wrap("00:00:00".into(), theme::mono(FONT_PT), theme::TEXT_FAINT).size().x + TIME_GAP } else { 0.0 };
    // leave room for the (floating) scrollbar so wrapped text never sits under it
    let text_w = (inner.width() - time_w - 10.0).max(24.0);

    let cache_id = egui::Id::new(("log_line_heights", salt));
    let mut heights: LineHeights = bui.data_mut(|d| d.remove_temp(cache_id)).unwrap_or_default();
    heights.refresh(&bui, lines, text_w);

    super::vscroll(touch)
        .id_salt(("log_scroll", salt))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show_viewport(&mut bui, |ui, viewport| {
            let total = heights.ends.last().copied().unwrap_or(0.0);
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), total), Sense::hover());
            let first = heights.ends.partition_point(|&e| e < viewport.min.y);
            for (i, l) in lines.iter().enumerate().skip(first) {
                let top = if i == 0 { 0.0 } else { heights.ends[i - 1] };
                if top > viewport.max.y {
                    break;
                }
                let line = Rect::from_min_max(pos2(area.left(), area.top() + top), pos2(area.right(), area.top() + heights.ends[i]));
                if times {
                    ui.painter().text(line.left_top(), egui::Align2::LEFT_TOP, &l.time, theme::mono(FONT_PT), theme::TEXT_FAINT);
                }
                let text_rect = Rect::from_min_size(pos2(line.left() + time_w, line.top()), vec2(text_w, line.height()));
                ui.scope_builder(
                    UiBuilder::new().id_salt(("log_line", i)).max_rect(text_rect).layout(Layout::top_down(Align::LEFT)),
                    |ui| {
                        ui.add(egui::Label::new(line_text(l)).wrap_mode(TextWrapMode::Wrap).selectable(true));
                    },
                );
            }
        });
    bui.data_mut(|d| d.insert_temp(cache_id, heights));
}
