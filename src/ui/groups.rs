//! Collapsible section groups + card grid + cards (**I9**).
//!
//! Mirrors the Graphite mockup (`design/ritz-get-gui-graphite.html`, `.group`, `.grid`, `.tile`):
//!
//! * **Group** = header row (chevron, accent UPPERCASE title, count, hairline rule) above a
//!   fluid grid ([`grid_columns`], [`grid_widths`]: cards between `MIN_CARD_W` and `MAX_CARD_W`
//!   wide, never more columns than the longest section has cards) with an 8 pt gap;
//!   12 pt between groups. The collapse state lives in `AppState::collapsed` (the header pushes
//!   `Action::ToggleGroup`).
//! * **Card** (`.tile`): 1 pt BORDER, radius 7, padding 12, 9 pt between rows, `overflow: hidden`.
//!   Rows: pills + `↗` (24) / icon 48 + name (bold 16) + description (13, 2 lines) / version line
//!   (2 lines, Geist Mono 12) / primary button + `…` (28). Cards in one grid row share the
//!   tallest card's height (CSS grid stretch), so each card is measured first, then painted.
//! * **Display scaling**: every number here is in logical points (`pixels_per_point` is egui's
//!   business). The grid's column edges and the row heights are snapped to whole physical pixels
//!   ([`snap`]), so 1 pt borders of neighbouring cards tile without seams at 1.25 / 1.5 / 2.0.
//! * Everything is a pane (`theme::columns_sized` per grid row, `Ui::shrink_clip_rect` per card):
//!   nothing is painted outside its slot.
//!
//! Pill mapping ([`crate::ui::pills`]), primary button (see [`primary`]) and the version line
//! ([`version_job`]) follow PORCELAIN `state`/`status` plus the session badges.
//!
//! No `ScrollArea` here: `ui::draw` wraps this pane in one.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, UiBuilder, vec2};

use crate::app::{AppState, CardProgress, Tone};
use crate::model::{Action, Entry, Kind, Modal, State, Status, Version};
use crate::theme;

/// `minmax(270pt, 1fr)` (was 248): 3 columns at 1280, 2 at 900 and at 600.
const MIN_CARD_W: f32 = 270.0;
/// Widest a card gets (+30 % over [`MIN_CARD_W`]): beyond that the extra space stays empty on the
/// right instead of stretching the cards (a lone Installed card must not become 1000 pt wide), and
/// 350 is also where a wide window's extra column appears without cards dropping below ~280.
pub(super) const MAX_CARD_W: f32 = 350.0;
/// `.tile` padding 12 + border 1 (was 9).
const CARD_INSET: f32 = 13.0;
/// Gap between the rows inside a card (was 6).
const ROW_SPACE: f32 = 9.0;
/// Space between groups (`.groups` `gap: 12px`).
const GROUP_GAP: f32 = 12.0;
/// `.gh` square (was 20).
const GH: f32 = 24.0;
/// Primary button / `…` height on a card (was `theme::CTRL_H` = 22). Cards only: the rest of the
/// app keeps its 22 pt controls.
const BTN_H: f32 = 28.0;
/// Card icon (was 36).
const ICON: f32 = 48.0;
/// Icon to text.
const ICON_GAP: f32 = 10.0;
/// Name (Geist Bold) / description / version line (Geist Mono) sizes (were 13 / 11 / 11).
const NAME_PT: f32 = 16.0;
const DESC_PT: f32 = 13.0;
const VER_PT: f32 = 12.0;
/// Card button label (was 13).
const BTN_PT: f32 = 14.0;
/// Name to description (was 2).
const TXT_GAP: f32 = 3.0;

/// Draw all groups.
pub fn draw(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) {
    let sections = state.visible_sections();
    if sections.is_empty() {
        empty(ui, state);
        return;
    }
    // Gaps are placed explicitly (nested-pane rule: no implicit stacking spacing).
    ui.spacing_mut().item_spacing.y = 0.0;
    // One column grid for all sections (aligned across groups), no wider than the longest one.
    let max_cols = sections.iter().map(|(_, e)| e.len()).max().unwrap_or(1);
    for (i, (section, entries)) in sections.iter().enumerate() {
        if i > 0 {
            ui.add_space(GROUP_GAP);
        }
        group(ui, state, &section.name, entries, max_cols, actions);
    }
}

/// `.empty`: centred DIM text, 40 px padding above and below. While the first `list` is still in
/// flight (nothing loaded yet) a placeholder is shown instead of "no match".
fn empty(ui: &mut egui::Ui, state: &AppState) {
    let nothing_loaded = state.listing.sections.is_empty();
    let (text, color) = if nothing_loaded && state.ritz_get_missing {
        ("Nothing to show: ritz-get was not found.", theme::DIM)
    } else if nothing_loaded && state.refused.is_some() {
        ("Nothing to show until ritz-get-gui is updated.", theme::DIM)
    } else if nothing_loaded && state.loading {
        ("Loading programs\u{2026}", theme::FAINT)
    } else if nothing_loaded {
        ("No programs to show yet.", theme::DIM)
    } else {
        ("No programs match. Clear the search or pick another filter.", theme::DIM)
    };
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        let w = ui.available_width();
        let galley = ui.painter().layout(text.to_owned(), theme::regular(13.0), color, w);
        let (rect, _) = ui.allocate_exact_size(vec2(w, galley.size().y), Sense::hover());
        let x = rect.center().x - galley.size().x * 0.5;
        ui.painter().galley(Pos2::new(x, rect.top()), galley, color);
    });
    ui.add_space(40.0);
}

// ---- Group ----------------------------------------------------------------

fn group(ui: &mut egui::Ui, state: &AppState, name: &str, entries: &[&Entry], max_cols: usize, actions: &mut Vec<Action>) {
    let collapsed = state.collapsed.contains(name);
    let head = group_head(ui, name, entries.len(), collapsed, actions);
    if collapsed {
        return;
    }
    ui.add_space(theme::GAP);

    let ppp = ui.ctx().pixels_per_point();
    let all = grid_widths(ui.available_width(), ppp, max_cols);
    let cols = all.len();
    for (ri, row) in entries.chunks(cols).enumerate() {
        if ri > 0 {
            ui.add_space(theme::GAP);
        }
        // A short last row keeps the same card widths (the first `row.len()` columns).
        let widths = &all[..row.len()];
        // CSS grid stretch: every card in the row is as tall as the tallest one (whole pixels).
        let h = row.iter().zip(widths).map(|(e, &w)| measure(ui, state, e, w)).fold(0.0_f32, f32::max);
        let h = snap_up(h, ppp);
        theme::columns_sized(ui, widths, theme::GAP, |c| {
            for (i, e) in row.iter().enumerate() {
                card(&mut c[i], state, e, widths[i], h, head, actions);
            }
        });
    }
}

/// Number of grid columns for a pane `w` points wide holding at most `max_cols` cards per row.
///
/// `min(fit, ceil((w+GAP)/(MAX+GAP)), max_cols)`, at least 1, where `fit` is how many
/// `MIN_CARD_W` cards fit. A column is added as soon as the cards would otherwise be wider than
/// `MAX_CARD_W`, or (when that would squeeze them below `MIN_CARD_W`, i.e. in the 1 to 3 column
/// range) as soon as it fits at `MIN_CARD_W`. The column cap keeps few cards from getting columns
/// that stay empty in every section.
pub(super) fn grid_columns(w: f32, max_cols: usize) -> usize {
    let per = |card: f32| (w + theme::GAP) / (card + theme::GAP);
    let fit = per(MIN_CARD_W).floor() as usize;
    let wanted = per(MAX_CARD_W).ceil() as usize;
    fit.min(wanted).min(max_cols).max(1)
}

/// `x` rounded to a whole physical pixel (`ppp` = pixels per point).
fn snap(x: f32, ppp: f32) -> f32 {
    (x * ppp).round() / ppp
}

/// `x` rounded up to a whole physical pixel.
fn snap_up(x: f32, ppp: f32) -> f32 {
    (x * ppp - 1e-3).ceil() / ppp
}

/// Card widths of one full grid row in a pane `w` points wide (see [`grid_columns`]): the cards
/// share the row evenly up to `MAX_CARD_W` each; capped cards leave the rest of the row empty
/// (left-aligned grid). Every column edge lies on a whole physical pixel, so the widths differ by
/// at most one pixel and neighbouring 1 pt borders never straddle a pixel boundary.
pub(super) fn grid_widths(w: f32, ppp: f32, max_cols: usize) -> Vec<f32> {
    let cols = grid_columns(w, max_cols);
    let col_w = ((w - theme::GAP * (cols as f32 - 1.0)) / cols as f32).clamp(0.0, MAX_CARD_W);
    (0..cols)
        .map(|i| {
            let left = i as f32 * (col_w + theme::GAP);
            snap(left + col_w, ppp) - snap(left, ppp)
        })
        .collect()
}

/// `.group-head`: chevron (12 px box) | TITLE | count | rule. The whole row toggles the group.
fn group_head(ui: &mut egui::Ui, name: &str, count: usize, collapsed: bool, actions: &mut Vec<Action>) -> Rect {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        actions.push(Action::ToggleGroup(name.to_owned()));
    }
    if !ui.is_rect_visible(rect) {
        return rect;
    }
    let p = ui.painter();
    let glyph = if collapsed { theme::glyph::CHEVRON_R } else { theme::glyph::CHEVRON };
    theme::paint_glyph(p, Pos2::new(rect.left() + 6.0, rect.center().y), glyph, 10.0, theme::TEXT_DIM);

    let font = theme::bold(13.0);
    let mut job = LayoutJob::default();
    job.append(
        &name.to_uppercase(),
        0.0,
        TextFormat {
            font_id: font.clone(),
            color: theme::ACCENT,
            extra_letter_spacing: font.size * 0.04,
            underline: if resp.hovered() { Stroke::new(1.0_f32, theme::ACCENT) } else { Stroke::NONE },
            ..Default::default()
        },
    );
    let title = p.layout_job(job);
    let tx = rect.left() + 12.0 + 8.0;
    let tw = title.size().x;
    let ty = rect.center().y - title.size().y * 0.5;
    p.galley(Pos2::new(tx, ty), title, theme::ACCENT);

    let count_g = p.layout_no_wrap(count.to_string(), theme::regular(12.0), theme::TEXT_FAINT);
    let cx = tx + tw + 8.0;
    let cw = count_g.size().x;
    p.galley(Pos2::new(cx, rect.center().y - count_g.size().y * 0.5), count_g, theme::TEXT_FAINT);

    let rx = cx + cw + 8.0;
    if rect.right() - rx >= 8.0 {
        // On the middle of a physical pixel row, whatever the display scale.
        let y = egui::emath::GuiRounding::round_to_pixel_center(rect.center().y, ui.ctx().pixels_per_point());
        p.hline(rx..=rect.right(), y, Stroke::new(1.0_f32, theme::BORDER));
    }
    rect
}

// ---- Card -------------------------------------------------------------------

/// Total outer height of `e`'s card at width `w` (content + actions row + insets).
fn measure(ui: &mut egui::Ui, state: &AppState, e: &Entry, w: f32) -> f32 {
    let origin = ui.cursor().min + vec2(CARD_INSET, CARD_INSET);
    let inner = Rect::from_min_size(origin, vec2((w - 2.0 * CARD_INSET).max(0.0), 10_000.0));
    let mut sink = Vec::new();
    let content = content(ui, state, e, inner, false, &mut sink);
    2.0 * CARD_INSET + content + ROW_SPACE + BTN_H
}

/// One `.tile`, `w` x `h` (h from [`measure`], maximised over the grid row).
fn card(ui: &mut egui::Ui, state: &AppState, e: &Entry, w: f32, h: f32, head: Rect, actions: &mut Vec<Action>) {
    let id = e.id();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let selected = state.selected.as_deref() == Some(id.as_str());
    let hovered = resp.contains_pointer();
    if resp.clicked() {
        actions.push(Action::Select(id.clone()));
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    // One-shot (selection changed / shot hook): bring the selected card into view. A card that is
    // already fully visible stays put, so clicking never recentres the pane. The group header
    // comes along (centred together with the card) whenever both fit in the pane.
    if selected && state.scroll_to_card {
        let clip = ui.clip_rect();
        if rect.top() < clip.top() || rect.bottom() > clip.bottom() {
            let with_head = rect.union(head);
            let target = if with_head.height() <= clip.height() { with_head } else { rect };
            ui.scroll_to_rect(target, Some(Align::Center));
        }
    }

    let (fill, border) = match (selected, hovered) {
        (true, true) => (theme::SEL, theme::ACCENT),
        (true, false) => (theme::SEL, theme::SELBD),
        (false, true) => (theme::HOV, theme::ACCENT),
        (false, false) => (Color32::TRANSPARENT, theme::BORDER),
    };
    ui.painter().rect(rect, CornerRadius::same(theme::RADIUS), fill, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    focus_ring(ui, &resp, rect, theme::RADIUS);

    let inner = rect.shrink(CARD_INSET);
    let mut cui = ui.new_child(
        UiBuilder::new().id_salt(("card", &id)).max_rect(inner).layout(Layout::top_down(Align::Min)),
    );
    cui.shrink_clip_rect(rect.shrink(1.0));
    content(&mut cui, state, e, inner, true, actions);

    // Actions row, pinned to the bottom (`margin-top: auto`).
    let row = Rect::from_min_size(Pos2::new(inner.left(), inner.bottom() - BTN_H), vec2(inner.width(), BTN_H));
    actions_row(&mut cui, state, e, row, actions);
}

/// Everything above the actions row. In measure mode (`paint == false`) nothing is painted and
/// the widgets are laid out invisibly. Returns the content height from `inner.top()`.
fn content(ui: &mut egui::Ui, state: &AppState, e: &Entry, inner: Rect, paint: bool, actions: &mut Vec<Action>) -> f32 {
    let left = inner.left();
    let mut y = inner.top();

    // -- pills | ↗ ------------------------------------------------------------
    let gh_url = (!e.is_unlisted()).then(|| e.repo_url()).flatten();
    let pills_w = if gh_url.is_some() { inner.width() - GH - 6.0 } else { inner.width() };
    let mut b = UiBuilder::new()
        .id_salt(("pills", paint))
        .max_rect(Rect::from_min_size(Pos2::new(left, y), vec2(pills_w.max(0.0), 400.0)))
        .layout(Layout::left_to_right(Align::Min).with_main_wrap(true));
    if !paint {
        b = b.invisible();
    }
    let mut pui = ui.new_child(b);
    pui.spacing_mut().item_spacing = vec2(4.0, 4.0);
    pills(&mut pui, state, e);
    let pills_h = pui.min_rect().height();

    if let (true, Some(url)) = (paint, gh_url) {
        let r = Rect::from_min_size(Pos2::new(inner.right() - GH, y), vec2(GH, GH));
        let resp = ui
            .scope_builder(UiBuilder::new().max_rect(r), |ui| theme::icon_button(ui, "\u{2197}", GH))
            .inner
            .on_hover_text("Open on GitHub");
        if resp.clicked() {
            actions.push(Action::OpenUrl(url));
        }
    }
    // The `.gh` square's height is reserved on every card (even without ↗), so cards line up.
    let row_h = pills_h.max(GH);
    y += row_h;
    y += ROW_SPACE;

    // -- icon | name + description -----------------------------------------------
    let tx = left + ICON + ICON_GAP;
    let tw = (inner.right() - tx).max(0.0);
    let p = ui.painter();
    let name = p.layout_job(text_job(e.display(), theme::bold(NAME_PT), theme::TEXT, tw, 2, NAME_PT * 1.25));
    let desc_h_min = 2.0 * DESC_PT * 1.4;
    let desc = (!e.desc.is_empty()).then(|| p.layout_job(text_job(&e.desc, theme::regular(DESC_PT), theme::DIM, tw, 2, DESC_PT * 1.4)));
    let desc_h = desc.as_ref().map_or(0.0, |g| g.size().y).max(desc_h_min);
    let txt_h = name.size().y + TXT_GAP + desc_h;
    // Without a description the name alone is centred against the icon.
    let name_dy = if desc.is_none() { ((ICON - name.size().y) * 0.5).max(0.0) } else { 0.0 };
    if paint {
        let r = Rect::from_min_size(Pos2::new(left, y), vec2(ICON, ICON));
        ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
            crate::icons::draw(ui, e, state.dir.as_deref(), ICON);
        });
        let p = ui.painter();
        let ny = y + name.size().y;
        p.galley(Pos2::new(tx, y + name_dy), name, theme::TEXT);
        if let Some(d) = desc {
            p.galley(Pos2::new(tx, ny + TXT_GAP), d, theme::DIM);
        }
    }
    y += txt_h.max(ICON) + ROW_SPACE;

    // -- version line (2 lines reserved) -----------------------------------------
    let ver = ui.painter().layout_job(version_job(ui.painter(), e, inner.width()));
    let ver_h = ver.size().y.max(2.0 * VER_PT * 1.45);
    if paint {
        ui.painter().galley(Pos2::new(left, y), ver, theme::TEXT_DIM);
    }
    y += ver_h;

    y - inner.top()
}

/// A wrapping text job with a fixed line height and an ellipsis after `rows` lines.
fn text_job(text: &str, font: FontId, color: Color32, width: f32, rows: usize, line_height: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: font, color, line_height: Some(line_height), ..Default::default() });
    job.wrap = TextWrapping { max_width: width, max_rows: rows, break_anywhere: false, overflow_character: Some('\u{2026}') };
    job
}

// ---- Pills ------------------------------------------------------------------

/// The pill row: the shared mapping of [`crate::ui::pills`] (also used by the side hero).
fn pills(ui: &mut egui::Ui, state: &AppState, e: &Entry) {
    for p in crate::ui::pills::pills(state, e) {
        crate::ui::pills::show(ui, &p);
    }
}

// ---- Version line -----------------------------------------------------------

/// `.ver`: Geist Mono 12, TEXT_DIM with `b` parts in TEXT and the arrow in WARN.
///
/// * website: short host (mockup `shortHost`; D9: no version)
/// * installed + update: `<installed> installed -> <latest> latest`
/// * installed: `<installed> · branch <branch>`
/// * not installed: `latest <sha>` (bare sha)
fn version_job(painter: &egui::Painter, e: &Entry, width: f32) -> LayoutJob {
    let font = theme::mono(VER_PT);
    let mut job = LayoutJob::default();
    let mut put = |text: &str, color: Color32| {
        job.append(
            text,
            0.0,
            TextFormat { font_id: font.clone(), color, line_height: Some(VER_PT * 1.45), ..Default::default() },
        );
    };
    // `b { white-space: nowrap }`: keep "date sha" together.
    let nb = |v: &Version| v.to_string().replace(' ', "\u{a0}");

    if e.kind == Kind::Website {
        if let Some(host) = e.website_url().map(short_host) {
            put(&elide_left(painter, &host, &font, width), theme::TEXT_DIM);
        }
    } else if e.is_installed() {
        if let Some(inst) = &e.installed {
            put(&nb(inst), theme::TEXT);
            match (&e.latest, e.behind_upstream()) {
                (Some(latest), true) => {
                    put(" installed ", theme::TEXT_DIM);
                    put("\u{2192}", theme::WARN);
                    put(" ", theme::TEXT_DIM);
                    put(&nb(latest), theme::TEXT);
                    put(" latest", theme::TEXT_DIM);
                }
                _ => {
                    if let Some(b) = &e.branch {
                        put(&format!(" \u{b7} branch {b}"), theme::TEXT_DIM);
                    }
                    if let Some(p) = &e.pending_branch {
                        put(" \u{2192}", theme::WARN);
                        put(&format!(" {p}"), theme::TEXT);
                    }
                }
            }
        }
    } else if let Some(latest) = &e.latest {
        put("latest ", theme::TEXT_DIM);
        put(&latest.sha, theme::TEXT);
    }
    job.wrap = TextWrapping { max_width: width, max_rows: 3, break_anywhere: false, overflow_character: Some('\u{2026}') };
    job
}

/// The URL without its scheme (`https://`): the side panel's full `url` row.
pub(super) fn strip_scheme(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, r)| r)
}

/// Compact card form of a website URL (scheme, query and fragment dropped):
///
/// * `host`, `host/` -> `host`
/// * `host/dir`, `host/dir/` -> `host/dir`
/// * `host/dir/page.html`, `host/a/b/c` -> `host/…/page`, `host/…/c`
///   (more than one path segment: only the last one is kept; a file loses its extension)
/// * `host/page.html` -> `host/page`
///
/// Website cards of one site then differ by their file name instead of all reading the same.
pub(super) fn short_host(url: &str) -> String {
    let rest = strip_scheme(url);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let dir_like = rest.ends_with('/');
    let mut parts = rest.split('/').filter(|s| !s.is_empty());
    let host = parts.next().unwrap_or("");
    let segs: Vec<&str> = parts.collect();
    let Some(&last) = segs.last() else { return host.to_owned() };
    let last = if dir_like { last } else { last.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()).map_or(last, |(stem, _)| stem) };
    if segs.len() > 1 { format!("{host}/\u{2026}/{last}") } else { format!("{host}/{last}") }
}

/// Fits `host/…/stem` into `max_w` by dropping whole segments from the left: the full form, then
/// `…/stem`, and only then the stem itself cut from the left (`…ing-generator`). `width` measures.
fn elide_left_with(text: &str, max_w: f32, width: &dyn Fn(&str) -> f32) -> String {
    if width(text) <= max_w {
        return text.to_owned();
    }
    let tail = text.rsplit('/').next().unwrap_or(text);
    if tail.len() < text.len() {
        let seg = format!("\u{2026}/{tail}");
        if width(&seg) <= max_w {
            return seg;
        }
    }
    for (i, _) in tail.char_indices().skip(1) {
        let cut = format!("\u{2026}{}", &tail[i..]);
        if width(&cut) <= max_w {
            return cut;
        }
    }
    "\u{2026}".to_owned()
}

fn elide_left(painter: &egui::Painter, text: &str, font: &egui::FontId, max_w: f32) -> String {
    elide_left_with(text, max_w, &|t| painter.layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE).size().x)
}

#[cfg(test)]
mod elide_tests {
    use super::elide_left_with;

    #[test]
    fn drops_segments_before_cutting_words() {
        let w = |t: &str| t.chars().count() as f32;
        let t = "host.io/\u{2026}/cs2-generator";
        assert_eq!(elide_left_with(t, 40.0, &w), t);
        assert_eq!(elide_left_with(t, 20.0, &w), "\u{2026}/cs2-generator");
        assert_eq!(elide_left_with(t, 10.0, &w), "\u{2026}generator");
        assert_eq!(elide_left_with("host.io/dir", 7.0, &w), "\u{2026}/dir");
        assert_eq!(elide_left_with("host.io", 3.0, &w), "\u{2026}io");
    }
}

#[cfg(test)]
mod short_host_tests {
    use super::{short_host, strip_scheme};

    #[test]
    fn shapes() {
        assert_eq!(short_host("https://example.org"), "example.org");
        assert_eq!(short_host("https://example.org/"), "example.org");
        assert_eq!(short_host("https://example.org/dir"), "example.org/dir");
        assert_eq!(short_host("https://example.org/dir/"), "example.org/dir");
        assert_eq!(short_host("https://example.org/dir/page.html"), "example.org/\u{2026}/page");
        assert_eq!(short_host("https://example.org/page.html"), "example.org/page");
        assert_eq!(short_host("https://example.org/a/b/c/page.v2.html"), "example.org/\u{2026}/page.v2");
        assert_eq!(short_host("https://example.org/a/b/c"), "example.org/\u{2026}/c");
        assert_eq!(short_host("https://example.org/a/b/c/"), "example.org/\u{2026}/c");
        assert_eq!(short_host("https://example.org/dir/page.html?x=1.5#top"), "example.org/\u{2026}/page");
        assert_eq!(short_host("https://example.org/dir?x=1/2#a/b"), "example.org/dir");
        assert_eq!(short_host("example.org/dir/page.html"), "example.org/\u{2026}/page");
    }

    #[test]
    fn fixture_websites_differ_by_file_name() {
        let a = short_host("https://ritze03.github.io/stupid-pages/cs2-multimap-generator.html");
        let b = short_host("https://ritze03.github.io/stupid-pages/cs2-chat-keybind-generator.html");
        assert_eq!(a, "ritze03.github.io/\u{2026}/cs2-multimap-generator");
        assert_ne!(a, b);
        assert_eq!(short_host("https://ritze03.github.io/simple-screensharing/"), "ritze03.github.io/simple-screensharing");
    }

    #[test]
    fn full_url_row_drops_only_the_scheme() {
        assert_eq!(strip_scheme("https://a.io/x/y.html?q=1"), "a.io/x/y.html?q=1");
        assert_eq!(strip_scheme("a.io/x"), "a.io/x");
    }
}

// ---- Actions row ------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Primary,
    Danger,
    Disabled,
}

struct Primary {
    label: &'static str,
    look: Look,
    action: Option<Action>,
    hint: Option<&'static str>,
}

/// The primary button (mockup `primary(e)`):
///
/// | entry                         | button                                                     |
/// |-------------------------------|------------------------------------------------------------|
/// | website                       | Open ↗ (`Action::Open`)                                    |
/// | unlisted                      | Remove (danger; the card's only action)                    |
/// | installed, update available   | Update (disabled when modified: updates are skipped)       |
/// | installed, current            | "Up to date" (disabled)                                    |
/// | installed otherwise           | "Installed" (disabled: unknown / fetch failed / modified)  |
/// | not installed, offline        | Install (disabled, "You're offline")                       |
/// | not installed                 | Install                                                    |
///
/// Every command button is drawn disabled unless `state.actions_enabled()`.
fn primary(state: &AppState, e: &Entry) -> Primary {
    let name = e.name.clone();
    let mut p = if e.is_website() {
        Primary { label: "Open \u{2197}", look: Look::Primary, action: Some(Action::Open { name }), hint: None }
    } else if e.is_unlisted() {
        Primary { label: "Remove", look: Look::Danger, action: Some(Action::Remove { name }), hint: None }
    } else if e.is_installed() {
        if e.has_update() && e.is_modified() {
            Primary {
                label: "Update",
                look: Look::Disabled,
                action: None,
                hint: Some("Local changes: updates are skipped until you reset"),
            }
        } else if e.has_update() {
            Primary { label: "Update", look: Look::Primary, action: Some(e.update_action()), hint: None }
        } else if e.status == Status::Current {
            Primary { label: "Up to date", look: Look::Disabled, action: None, hint: None }
        } else {
            Primary { label: "Installed", look: Look::Disabled, action: None, hint: None }
        }
    } else if e.state == State::Offline {
        Primary { label: "Install", look: Look::Disabled, action: None, hint: Some("You're offline") }
    } else {
        Primary { label: "Install", look: Look::Primary, action: Some(e.install_action()), hint: None }
    };
    if !state.actions_enabled() && p.look != Look::Disabled {
        p.look = Look::Disabled;
        p.action = None;
    }
    p
}

fn actions_row(ui: &mut egui::Ui, state: &AppState, e: &Entry, row: Rect, actions: &mut Vec<Action>) {
    let id = e.id();
    let has_more = !e.is_unlisted(); // D54: Unlisted cards offer Remove only
    let more_w = if has_more { BTN_H + 6.0 } else { 0.0 };
    let prim_rect = Rect::from_min_size(row.min, vec2((row.width() - more_w).max(0.0), BTN_H));

    if let Some(cp) = state.progress.get(&e.name) {
        // A running / just finished install, update, ...: the button is its progress bar instead.
        // Not clickable (a click falls through to the card = select), except while the run's dialog
        // is hidden: then the bar is the way back into it.
        let back = state.run_hidden();
        let resp = ui.scope_builder(UiBuilder::new().max_rect(prim_rect), |ui| progress_bar(ui, cp, back)).inner;
        if back && resp.clicked() {
            actions.push(Action::Select(id.clone()));
            actions.push(Action::ShowModal(Modal::Run));
        }
    } else {
        let prim = primary(state, e);
        let resp = ui
            .scope_builder(UiBuilder::new().max_rect(prim_rect), |ui| flat_button(ui, prim.label, prim.look))
            .inner;
        let resp = match prim.hint {
            Some(h) => resp.on_hover_text(h),
            None => resp,
        };
        if resp.clicked() {
            if let Some(a) = prim.action {
                actions.push(Action::Select(id.clone()));
                actions.push(a);
            }
        }
    }

    if has_more {
        let r = Rect::from_min_size(Pos2::new(row.right() - BTN_H, row.top()), vec2(BTN_H, BTN_H));
        let more = ui.scope_builder(UiBuilder::new().max_rect(r), |ui| more_button(ui, BTN_H)).inner;
        if more.clicked() {
            actions.push(Action::Select(id.clone()));
        }
        // Screenshot hook (D57): `menu=<id>` opens this popover on the first frame it is set.
        if state.open_menu.as_deref() == Some(id.as_str()) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&more));
        }
        let frame = egui::Frame::new()
            .fill(theme::FIELD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .corner_radius(theme::RADIUS)
            .inner_margin(4)
            .shadow(egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: theme::with_alpha(theme::HEAD, 0.72),
            });
        egui::Popup::menu(&more)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(4.0)
            .frame(frame)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| popover(ui, state, e, actions));
    }
}

/// The `...` popover (mockup `.pop`): name head, separator, Branch (expandable) + Auto-update
/// rows for programs, separator, then I10's shared action list. Picking an action closes it.
fn popover(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    const W: f32 = 240.0;
    ui.set_width(W);
    ui.spacing_mut().item_spacing.y = 1.0;
    // `.pop-head { padding: 4px 6px 2px }`
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.add(egui::Label::new(egui::RichText::new(e.display()).font(theme::bold(12.0)).color(theme::TEXT)).wrap());
    });
    ui.add_space(2.0);
    separator(ui);
    if !e.is_website() {
        branch_item(ui, state, e, actions);
        auto_update_item(ui, state, e, actions);
        separator(ui);
    }
    let before = actions.len();
    crate::ui::side::action_list(ui, state, e, actions);
    if actions.len() > before {
        ui.close();
    }
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(3.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::BORDER);
    ui.add_space(3.0);
}

/// `Branch  > main`: toggles the branch list (asked for once, when first expanded).
fn branch_item(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    let open_id = egui::Id::new(("pop_branch_open", e.id()));
    let open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(theme::RADIUS), theme::HOV);
    }
    let glyph = if open { theme::glyph::CHEVRON } else { theme::glyph::CHEVRON_R };
    theme::paint_glyph(p, Pos2::new(rect.left() + 14.0, rect.center().y), glyph, 10.0, theme::TEXT_DIM);
    p.text(Pos2::new(rect.left() + 30.0, rect.center().y), egui::Align2::LEFT_CENTER, "Branch", theme::regular(13.0), theme::TEXT);
    if let Some(b) = e.pending_branch.as_ref().or(e.branch.as_ref()) {
        p.text(Pos2::new(rect.right() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, b, theme::mono(11.0), theme::TEXT_FAINT);
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(open_id, !open));
        if !open && !state.branches.contains_key(&e.name) {
            actions.push(Action::Branches { name: e.name.clone() });
        }
    }
    if !open {
        return;
    }
    // The picked-but-not-applied branch is the selected one; the checked-out one is tagged below.
    let installed_branch = e.branch.clone().unwrap_or_default();
    let cur = e.pending_branch.clone().unwrap_or_else(|| installed_branch.clone());
    let list_item = |ui: &mut egui::Ui, label: String, enabled: bool| -> bool {
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), sense);
        let rect = Rect::from_min_max(Pos2::new(rect.left() + 22.0, rect.top()), rect.max);
        let color = if enabled { theme::TEXT } else { theme::FAINT };
        if enabled && resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS), theme::HOV);
        }
        ui.painter().text(Pos2::new(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::regular(13.0), color);
        enabled && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    match state.branches.get(&e.name) {
        None => {
            list_item(ui, "Loading\u{2026}".to_owned(), false);
        }
        Some(list) => {
            for (b, _) in list {
                let mut label = format!("{} {b}", if *b == cur { "\u{25cf}" } else { "\u{25cb}" });
                if e.pending_branch.is_some() && *b == installed_branch {
                    label.push_str("  (installed)");
                }
                if list_item(ui, label, state.actions_enabled()) && *b != cur {
                    actions.push(Action::PickBranch { name: e.name.clone(), branch: b.clone() });
                    ui.close();
                }
            }
        }
    }
}

/// `Auto-update [switch]` (on = not held), enabled only for installed programs.
fn auto_update_item(ui: &mut egui::Ui, state: &AppState, e: &Entry, actions: &mut Vec<Action>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let p = ui.painter();
    theme::paint_glyph(p, Pos2::new(rect.left() + 14.0, rect.center().y), theme::glyph::REFRESH, 13.0, theme::TEXT_DIM);
    p.text(Pos2::new(rect.left() + 30.0, rect.center().y), egui::Align2::LEFT_CENTER, "Auto-update", theme::regular(13.0), theme::TEXT);
    let sw = Rect::from_min_size(Pos2::new(rect.right() - 6.0 - 34.0, rect.center().y - 9.0), vec2(34.0, 18.0));
    let enabled = e.is_installed() && state.actions_enabled();
    let r = ui.scope_builder(UiBuilder::new().max_rect(sw), |ui| crate::ui::side::auto_switch(ui, e.is_installed() && !e.held, enabled)).inner;
    if r.clicked() {
        let name = e.name.clone();
        actions.push(if e.held { Action::Unhold { name } } else { Action::Hold { name } });
    }
}

/// `:focus-visible { outline: 1px solid accent; outline-offset: 1px }` (mockup l.61). egui buttons
/// only take the focus from the keyboard (Tab), which is exactly `:focus-visible`.
pub(super) fn focus_ring(ui: &egui::Ui, resp: &egui::Response, rect: Rect, radius: u8) {
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(radius.saturating_add(1)),
            Stroke::new(1.0_f32, theme::ACCENT),
            StrokeKind::Outside,
        );
    }
}

/// The `…` square of a card: `theme::icon_button`'s look with its three dots scaled to the
/// button (the theme's are drawn for a 22 pt square and read as specks at [`BTN_H`]).
fn more_button(ui: &mut egui::Ui, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let border = if !enabled {
            theme::BORDER
        } else if resp.is_pointer_button_down_on() {
            theme::SELBD
        } else if resp.hovered() {
            theme::ACCENT
        } else {
            theme::BTNBD
        };
        let p = ui.painter();
        p.rect(rect, CornerRadius::same(theme::RADIUS), theme::BTN, Stroke::new(1.0_f32, border), StrokeKind::Inside);
        if enabled && resp.hovered() {
            p.rect_filled(rect.shrink(1.0), CornerRadius::same(theme::RADIUS - 1), theme::HOV);
        }
        let k = side / theme::CTRL_H;
        let c = if enabled { theme::TEXT } else { theme::FAINT };
        for dx in [-3.6_f32, 0.0, 3.6] {
            p.circle_filled(rect.center() + vec2(dx * k, k), 0.85 * k, c);
        }
    }
    if ui.is_enabled() { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// `.btn` ([`BTN_H`] high, radius 7) in the primary / danger / disabled look. Fills the `Ui`'s rect.
fn flat_button(ui: &mut egui::Ui, label: &str, look: Look) -> egui::Response {
    let size = vec2(ui.max_rect().width(), BTN_H);
    let enabled = look != Look::Disabled;
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let (fill, mut border, text, font) = match look {
            Look::Primary => (theme::ACCENT, theme::ACCENT, theme::PRIMARY_TEXT, theme::bold(BTN_PT)),
            Look::Danger => (
                Color32::TRANSPARENT,
                theme::with_alpha(theme::DANGER, 0.42),
                theme::DANGER,
                theme::regular(BTN_PT),
            ),
            Look::Disabled => (theme::BTN, theme::BORDER, theme::FAINT, theme::regular(BTN_PT)),
        };
        let p = ui.painter();
        if enabled && resp.is_pointer_button_down_on() {
            border = theme::SELBD;
        } else if enabled && resp.hovered() {
            border = if look == Look::Danger { theme::DANGER } else { theme::ACCENT };
        }
        p.rect(rect, CornerRadius::same(theme::RADIUS), fill, Stroke::new(1.0_f32, border), StrokeKind::Inside);
        if enabled && resp.hovered() {
            p.rect_filled(rect.shrink(1.0), CornerRadius::same(theme::RADIUS - 1), theme::HOV);
        }
        let g = p.layout_no_wrap(label.to_owned(), font, text);
        p.galley(rect.center() - g.size() * 0.5, g, text);
        if enabled {
            focus_ring(ui, &resp, rect, theme::RADIUS);
        }
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

// ---- Progress bar -----------------------------------------------------------

/// Seconds for the sweep to go there and back.
const SWEEP_PERIOD: f64 = 1.7;
/// The sweeping segment's share of the track.
const SWEEP_FRAC: f32 = 0.38;

/// (fill, border, text) of a bar in `tone`.
pub(super) fn tone_colors(tone: Tone) -> (Color32, Color32, Color32) {
    match tone {
        Tone::Busy => (theme::FIELD, theme::with_alpha(theme::ACCENT, 0.6), theme::TEXT),
        Tone::Queued => (theme::FIELD, theme::BORDER, theme::TEXT_DIM),
        Tone::Good => (theme::with_alpha(theme::GOOD, 0.14), theme::with_alpha(theme::GOOD, 0.45), theme::GOOD),
        // calmer than Updated, but visibly a result (not the idle disabled "Up to date" button)
        Tone::Even => (theme::with_alpha(theme::GOOD, 0.07), theme::with_alpha(theme::GOOD, 0.3), theme::TEXT),
        Tone::Warn => (theme::with_alpha(theme::WARN, 0.12), theme::with_alpha(theme::WARN, 0.42), theme::WARN),
        Tone::Danger => (theme::with_alpha(theme::DANGER, 0.14), theme::with_alpha(theme::DANGER, 0.5), theme::DANGER),
    }
}

/// The indeterminate sweep: a segment of [`SWEEP_FRAC`] of `track` that eases back and forth
/// between its ends (always fully inside, so the rounded track never needs clipping). `time` in seconds.
pub(super) fn sweep_rect(track: Rect, time: f64) -> Rect {
    let phase = (time / SWEEP_PERIOD).fract() as f32;
    let tri = 1.0 - (2.0 * phase - 1.0).abs();
    let eased = tri * tri * (3.0 - 2.0 * tri);
    let w = track.width() * SWEEP_FRAC;
    let x = track.left() + (track.width() - w) * eased;
    Rect::from_min_size(Pos2::new(x, track.top()), vec2(w, track.height()))
}

/// The card's primary button turned into a bar: same size ([`BTN_H`], the `Ui`'s width), radius 7,
/// 1 pt border. Running = an ACCENT sweep over a FIELD track (no percentage exists), Queued = the
/// still track, then the result in GOOD / GOOD-tinted (up to date) / WARN / DANGER, each with a glyph. The label sits inside, centred.
fn progress_bar(ui: &mut egui::Ui, cp: &CardProgress, clickable: bool) -> egui::Response {
    let sense = if clickable { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.max_rect().width(), BTN_H), sense);
    let resp = if clickable { resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show progress") } else { resp };
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let tone = cp.tone();
    let (fill, border, text) = tone_colors(tone);
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(theme::RADIUS), fill, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    if tone == Tone::Busy {
        let t = ui.ctx().input(|i| i.time);
        let seg = sweep_rect(rect.shrink(1.0), t);
        p.rect_filled(seg, CornerRadius::same(theme::RADIUS - 1), theme::with_alpha(theme::ACCENT, 0.62));
    }
    let glyph = match tone {
        Tone::Good | Tone::Even => Some(theme::glyph::CHECK),
        Tone::Danger => Some(theme::glyph::WARNING),
        Tone::Warn => Some(theme::glyph::PAUSE),
        _ => None,
    };
    let glyph_col = if tone == Tone::Even { theme::GOOD } else { text };
    let g = p.layout_no_wrap(cp.label().to_owned(), theme::bold(BTN_PT), text);
    let gap = if glyph.is_some() { 14.0 + 6.0 } else { 0.0 };
    let x0 = rect.center().x - (g.size().x + gap) * 0.5;
    if let Some(gl) = glyph {
        theme::paint_glyph(p, Pos2::new(x0 + 7.0, rect.center().y), gl, 12.0, glyph_col);
    }
    p.galley(Pos2::new(x0 + gap, rect.center().y - g.size().y * 0.5), g, text);
    resp
}

#[cfg(test)]
mod grid_rule_tests {
    use super::{grid_columns, grid_widths, MAX_CARD_W, MIN_CARD_W};
    use crate::theme::GAP;

    /// Few cards + a wide pane: one column per card, each exactly `MAX_CARD_W`, the rest empty.
    #[test]
    fn few_cards_stop_at_the_cap() {
        for ppp in [1.0_f32, 1.25, 1.5, 2.0] {
            for n in 1..=4usize {
                for w in [1500.0_f32, 1900.0, 2500.0] {
                    let ws = grid_widths(w, ppp, n);
                    assert_eq!(ws.len(), n, "{n} cards @ {w}: {ws:?}");
                    for c in &ws {
                        assert!((c - MAX_CARD_W).abs() <= 1.0 / ppp, "{n} cards @ {w} x{ppp}: {ws:?}");
                    }
                    let used = ws.iter().sum::<f32>() + GAP * (n as f32 - 1.0);
                    assert!(used < w, "row must leave empty space: {used} of {w}");
                }
            }
        }
    }

    /// Many cards: columns by fit, widths between the limits, the row filled whenever uncapped.
    #[test]
    fn many_cards_fill_by_fit() {
        for w in [270.0_f32, 560.0, 600.0, 880.0, 1250.0, 1500.0, 2100.0, 3000.0] {
            let ws = grid_widths(w, 1.0, 100);
            let fit = (((w + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
            assert!(ws.len() <= fit, "{w}: {} cols > fit {fit}", ws.len());
            for c in &ws {
                assert!(*c <= MAX_CARD_W + 1.0, "{w}: {ws:?}");
                assert!(w < MIN_CARD_W || *c >= MIN_CARD_W - 1.0, "{w}: {ws:?}");
            }
            let used = ws.iter().sum::<f32>() + GAP * (ws.len() as f32 - 1.0);
            if ws[0] < MAX_CARD_W - 1.0 {
                assert!((used - w).abs() <= 1.0, "{w}: uncapped row must fill: {used} {ws:?}");
            }
        }
        assert_eq!(grid_columns(1250.0, 100), 4);
        assert_eq!(grid_columns(580.0, 100), 2);
    }

    /// Widening the pane never shrinks the cards, except in the step that adds a column (which
    /// keeps them at least `MIN_CARD_W`); columns never go away. With the column cap (the card
    /// count) reached, the width is monotone, full stop.
    #[test]
    fn widening_never_shrinks_cards_except_when_a_column_appears() {
        for max_cols in [1usize, 2, 3, 4, 6, 100] {
            let mut prev = (0usize, 0.0_f32);
            let mut w = 280.0_f32;
            while w <= 3200.0 {
                let ws = grid_widths(w, 1.0, max_cols);
                let (cols, cw) = (ws.len(), ws[0]);
                assert!(cols >= prev.0, "columns went down: {} -> {cols} @ {w}", prev.0);
                if cols == prev.0 {
                    assert!(cw >= prev.1 - 1.0, "cap {max_cols}: width {} -> {cw} @ {w} with {cols} cols", prev.1);
                } else {
                    assert!(cw >= MIN_CARD_W - 1.0, "cap {max_cols}: {cw} @ {w}, {cols} cols");
                }
                prev = (cols, cw);
                w += 1.0;
            }
        }
    }
}
