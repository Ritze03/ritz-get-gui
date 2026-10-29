//! Top bar + restart bar (**I8**; D6 D25 D42 D43), canvas design "5d: pills + underline tabs".
//!
//! Row 1 (30 px): the wordmark on the left, the search field as a pill centred in the *window*
//! (so it does not move when the right group changes width), and on the right a joined capsule
//! [Check for updates | Update all (N)] and a round gear. Row 2 (36 px): underline tabs
//! All / Installed / Updates / Not installed with count badges (the selected one has a 2 px ACCENT
//! underline sitting on the bar's bottom hairline), the ghost "Sort <current>" button at the right
//! end, its chevron aligned with the gear's right edge. Every item is painted into an exact rect
//! computed by [`plan`], so nothing can paint outside the pane.
//!
//! Two forms ([`plan`]): the *wide* one above, used while the search can still be centred with at
//! least [`SEARCH_MIN_CENTRED`] px between the wordmark and the capsule; otherwise the *narrow*
//! one: row 1 = wordmark + capsule (Check for updates as icon only) + gear, the search pill on a
//! row of its own, then the tabs (tighter gap) with the Sort button reduced to its icon.

use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2};

use crate::app::AppState;
use crate::model::{Action, Filter, Modal, Sort, View};
use crate::theme::{self, glyph};

/// Row gap and item gap (canvas `gap: 8px`).
const GAP: f32 = theme::GAP;
/// Row 1: the pills, the capsule and the gear are all this high.
pub const ROW1_H: f32 = 30.0;
/// The search pill is as high as the other row-1 controls.
pub const SEARCH_H: f32 = ROW1_H;
/// Row 2 (tabs).
const ROW2_H: f32 = 36.0;
/// Preferred width of the search pill.
const SEARCH_W: f32 = 460.0;
/// Narrowest search pill that is still kept centred in the window (below this: narrow form).
const SEARCH_MIN_CENTRED: f32 = 150.0;
const GEAR: f32 = ROW1_H;
/// The wordmark, with the app's own icon (`BRAND_ICON` px square) and a gap in front of it.
const BRAND: &str = "Ritz-Get-GUI";
const BRAND_ICON: f32 = 22.0;
const BRAND_GAP: f32 = 8.0;
/// Canvas padding of a capsule half.
const HALF_PAD: f32 = 12.0;
/// Icon-only capsule half (canvas `padding: 0 10px` around a 14 px icon).
const ICON_HALF: f32 = 34.0;
/// The ghost Sort button: height, side padding, and the width of its icon-only form.
const SORT_H: f32 = 28.0;
const SORT_PAD: f32 = 6.0;
const SORT_ICON_W: f32 = 28.0;
/// Gap between the tabs (wide / narrow form; shrinks when the row is too tight).
const TAB_GAP_WIDE: f32 = 20.0;
const TAB_GAP_NARROW: f32 = 16.0;
/// Hairline at the bottom of the bar (the panel's bottom margin, see `ui::draw_panes`).
pub const HAIRLINE: f32 = 1.0;
/// The bar's top padding (canvas `padding: 10px 16px 0`; the side inset stays the app's 8 pt margin).
pub const PAD_TOP: i8 = 10;
/// How far the bar's items may reach into the panel margins (the Sort ghost's hover wash).
pub const CLIP_SLACK: f32 = 8.0;

fn text_w(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

/// Width of a mockup `.btn` with `text` (padding 9, 1 px border).
fn btn_w(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    text_w(ui, text, font).ceil() + 18.0 + 2.0
}

/// Canvas `.badge`: min 18 x 18, padding 0 5, 11 px medium, fully round.
fn badge_w(ui: &Ui, text: &str) -> f32 {
    (text_w(ui, text, theme::medium(11.0)).ceil() + 10.0).max(18.0)
}

/// Widths that decide the layout.
#[derive(Clone, Debug)]
struct Metrics {
    w: f32,
    /// x of the window centre (the panel's margins are symmetric).
    centre_x: f32,
    brand: f32,
    /// Check half: with its label / icon only.
    check_full: f32,
    check_icon: f32,
    upd: f32,
    /// Tab widths (padding and badge included; the first has no left padding).
    tabs: [f32; 4],
    /// Sort ghost with its current value; the widest it can get (decides the form).
    sort_full: f32,
    sort_max: f32,
}

/// The rect of every item and the total height.
#[derive(Clone, Debug)]
struct Plan {
    /// Narrow form (search on its own row, icon-only Check and Sort).
    narrow: bool,
    brand: Rect,
    search: Rect,
    /// The joined capsule (outer border box) and its two halves (inside the border).
    capsule: Rect,
    check: Rect,
    upd: Rect,
    gear: Rect,
    row2: Rect,
    tabs: [Rect; 4],
    sort: Rect,
    sort_icon_only: bool,
    /// Content height (row 1 [+ search row] + gap + row 2); the hairline sits right below.
    height: f32,
}

fn plan(m: &Metrics, origin: Pos2) -> Plan {
    let (left, right) = (origin.x, origin.x + m.w);
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(x, y), vec2(w, h));

    // ---- row 1, right group: capsule | gear ----
    let gear = at(right - GEAR, origin.y, GEAR, GEAR);
    let capsule_for = |check: f32| {
        let w = 1.0 + check + 1.0 + m.upd + 1.0;
        at(gear.left() - GAP - w, origin.y, w, ROW1_H)
    };
    // ---- wide form if the search can be centred between the wordmark and the capsule ----
    let lo = left + m.brand + GAP;
    let wide_capsule = capsule_for(m.check_full);
    let hi = wide_capsule.left() - GAP;
    let cw = (2.0 * (m.centre_x - lo).min(hi - m.centre_x)).min(SEARCH_W);
    let narrow = cw < SEARCH_MIN_CENTRED;
    let (capsule, check_w) = if narrow { (capsule_for(m.check_icon), m.check_icon) } else { (wide_capsule, m.check_full) };
    let check = at(capsule.left() + 1.0, capsule.top() + 1.0, check_w, ROW1_H - 2.0);
    let upd = at(check.right() + 1.0, check.top(), m.upd, ROW1_H - 2.0);
    let brand = at(left, origin.y + (ROW1_H - 22.0) / 2.0, m.brand, 22.0);

    let mut y = ROW1_H;
    let search = if narrow {
        // Own row: the 2 px canvas margin on top of the gap.
        y += GAP + 2.0;
        let r = at(left, origin.y + y, m.w, SEARCH_H);
        y += SEARCH_H;
        r
    } else {
        at(m.centre_x - cw / 2.0, origin.y, cw, SEARCH_H)
    };

    // ---- row 2: underline tabs + the Sort ghost at the right end ----
    y += GAP;
    let row2 = at(left, origin.y + y, m.w, ROW2_H);
    let sort_icon_only = narrow;
    let tabs_total: f32 = m.tabs.iter().sum();
    let sort_foot = if sort_icon_only { SORT_ICON_W } else { m.sort_max };
    let mut gap = if narrow { TAB_GAP_NARROW } else { TAB_GAP_WIDE };
    // Tighten the tab gap before anything would run into the Sort button.
    while gap > 8.0 && tabs_total + 3.0 * gap + GAP + sort_foot - 2.0 * SORT_PAD > m.w {
        gap -= 4.0;
    }
    let mut x = left;
    let mut tabs = [Rect::NOTHING; 4];
    for (t, w) in tabs.iter_mut().zip(m.tabs) {
        *t = at(x, row2.top(), w, ROW2_H);
        x += w + gap;
    }
    // The chevron / icon ink ends at the gear's right edge: the (transparent) button reaches past it.
    let sort_w = if sort_icon_only { SORT_ICON_W } else { m.sort_full };
    let sort_right = right + if sort_icon_only { (SORT_ICON_W - 15.0) / 2.0 } else { SORT_PAD };
    let sort = at(sort_right - sort_w, row2.top() + (ROW2_H - SORT_H) / 2.0, sort_w, SORT_H);
    y += ROW2_H;
    Plan { narrow, brand, search, capsule, check, upd, gear, row2, tabs, sort, sort_icon_only, height: y }
}

/// Fill the top-bar panel (HEAD fill and margins are already drawn by `ui::draw`).
pub fn draw(ui: &mut Ui, state: &AppState, actions: &mut Vec<Action>) {
    let w = ui.available_width().max(0.0);
    let settings = state.view == View::Settings;

    // ---- measure ----
    let brand_w = BRAND_ICON + BRAND_GAP + text_w(ui, BRAND, theme::bold(16.0)).ceil();
    let counts: Vec<String> = Filter::ALL.iter().map(|&f| state.counts().get(f).to_string()).collect();
    let mut tabs = [0.0_f32; 4];
    for (i, f) in Filter::ALL.iter().enumerate() {
        let pad_l = if i == 0 { 0.0 } else { 2.0 };
        tabs[i] = pad_l + text_w(ui, f.label(), theme::medium(13.0)).ceil() + 6.0 + badge_w(ui, &counts[i]) + 2.0;
    }
    let check_text = text_w(ui, "Check for updates", theme::regular(13.0)).max(text_w(ui, "Checking…", theme::regular(13.0))).ceil();
    // While Update all runs in the background this half is the way back into its dialog.
    let reopen = state.run.as_ref().filter(|r| r.is_all()).and_then(|_| state.reopen_label());
    let n = state.update_all_count();
    let upd_text = text_w(ui, "Update all", theme::bold(13.0)).ceil();
    let upd_badge = badge_w(ui, &n.to_string()).max(badge_w(ui, "00"));
    let upd_normal = 2.0 * HALF_PAD + 14.0 + 6.0 + upd_text + 6.0 + upd_badge;
    let upd_w = upd_normal.max(reopen.as_deref().map_or(0.0, |l| 2.0 * HALF_PAD + text_w(ui, l, theme::bold(13.0)).ceil()));
    let sort_w = |label: &str| {
        // icon, gap, "Sort", gap, value, gap, chevron
        2.0 * SORT_PAD + 14.0 + 6.0 + text_w(ui, "Sort", theme::regular(13.0)).ceil() + 6.0 + text_w(ui, label, theme::medium(13.0)).ceil() + 6.0 + 12.0
    };
    let origin = ui.cursor().min;
    let m = Metrics {
        w,
        centre_x: origin.x + w / 2.0,
        brand: brand_w,
        check_full: 2.0 * HALF_PAD + 14.0 + 6.0 + check_text,
        check_icon: ICON_HALF,
        upd: upd_w,
        tabs,
        sort_full: sort_w(state.sort.label()),
        sort_max: Sort::ALL.iter().map(|s| sort_w(s.label())).fold(0.0_f32, f32::max),
    };
    let p = plan(&m, origin);
    ui.allocate_exact_size(vec2(w, p.height), Sense::hover());
    // The panel is sized from last frame's content; wrapping changes the height, so settle it.
    let hid = Id::new("topbar_height");
    if ui.data_mut(|d| d.get_temp::<f32>(hid)).is_none_or(|h| (h - p.height).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(hid, p.height));
        ui.ctx().request_repaint();
    }

    // ---- paint ----
    // The bar's bottom hairline spans the whole window; the selected tab's underline overlaps it.
    let layer = ui.ctx().layer_painter(ui.layer_id());
    let reach = f32::from(theme::MARGIN);
    layer.rect_filled(
        Rect::from_min_max(Pos2::new(origin.x - reach, p.row2.bottom()), Pos2::new(origin.x + w + reach, p.row2.bottom() + HAIRLINE)),
        0.0,
        theme::BORDER,
    );
    if let Some(tex) = crate::icons::own_texture(ui.ctx(), BRAND_ICON) {
        let r = Rect::from_min_size(Pos2::new(p.brand.left(), p.brand.center().y - BRAND_ICON / 2.0), vec2(BRAND_ICON, BRAND_ICON));
        egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), r.size())).paint_at(ui, r);
    }
    let text_pos = Pos2::new(p.brand.left() + BRAND_ICON + BRAND_GAP, p.brand.center().y);
    ui.painter().text(text_pos, Align2::LEFT_CENTER, BRAND, theme::bold(16.0), theme::ACCENT);
    search_box(ui, p.search, state, actions);
    capsule(ui, &p, state, reopen.as_deref(), actions);
    let r = gear(ui, p.gear, settings);
    if r.clicked() {
        actions.push(if settings { Action::ShowMain } else { Action::ShowSettings });
    }
    tabs_row(ui, &layer, &p, state, &counts, actions);
    sort_button(ui, &p, state, actions);
}

// ---- row 1: the capsule ------------------------------------------------------------------------

/// Canvas `.cap2`: BTN pill with a BTNBD outline and a 1 px divider; the halves are
/// Check for updates and Update all. The Update all half is the primary (ACCENT) one while there
/// is something to update; muted (OFF text) otherwise.
fn capsule(ui: &mut Ui, p: &Plan, state: &AppState, reopen: Option<&str>, actions: &mut Vec<Action>) {
    let r = p.capsule;
    let rad = (r.height() / 2.0) as u8;
    ui.painter().rect_filled(r, CornerRadius::same(rad), theme::BTN);
    let inner = rad - 1;
    let left_c = CornerRadius { nw: inner, sw: inner, ne: 0, se: 0 };
    let right_c = CornerRadius { ne: inner, se: inner, nw: 0, sw: 0 };

    // ---- Check for updates ----
    let busy = state.loading;
    let check_on = state.actions_enabled() && !busy;
    let resp = half_bg(ui, p.check, left_c, "topbar_check", check_on, false);
    let col = if check_on { theme::TEXT } else { theme::OFF };
    let label = if busy { "Checking…" } else { "Check for updates" };
    if p.narrow {
        theme::paint_glyph(ui.painter(), p.check.center(), glyph::REFRESH, 13.0, col);
    } else {
        icon_label(ui, p.check, glyph::REFRESH, label, theme::regular(13.0), col, None);
    }
    let resp = if p.narrow { resp.on_hover_text(label) } else { resp };
    if resp.clicked() && check_on {
        actions.push(Action::CheckForUpdates);
    }

    // ---- Update all (N) ----
    if let Some(label) = reopen {
        let resp = half_bg(ui, p.upd, right_c, "topbar_updall", true, true).on_hover_text("Show the update again");
        ui.painter().text(p.upd.center(), Align2::CENTER_CENTER, label, theme::bold(13.0), theme::PRIMARY_TEXT);
        if resp.clicked() {
            actions.push(Action::ShowModal(Modal::Run));
        }
    } else {
        let n = state.update_all_count();
        let on = n > 0 && state.actions_enabled();
        let mut resp = half_bg(ui, p.upd, right_c, "topbar_updall", on, true);
        let (col, font, badge_fill) = if on {
            (theme::PRIMARY_TEXT, theme::bold(13.0), theme::with_alpha(theme::PRIMARY_TEXT, 0.18))
        } else {
            (theme::OFF, theme::regular(13.0), theme::HEAD)
        };
        icon_label(ui, p.upd, glyph::DOWNLOAD, "Update all", font, col, Some((n.to_string(), badge_fill, col)));
        let skipped = state.update_all_skipped();
        if skipped > 0 {
            resp = resp.on_hover_text(format!("{skipped} held or modified update{} skipped", if skipped == 1 { "" } else { "s" }));
        }
        if resp.clicked() && on {
            actions.push(Action::Update { name: None });
        }
    }

    // ---- divider + outline (over the halves, like the canvas border) ----
    let sep = Rect::from_min_max(Pos2::new(p.check.right(), r.top() + 1.0), Pos2::new(p.upd.left(), r.bottom() - 1.0));
    ui.painter().rect_filled(sep, 0.0, theme::BTNBD);
    ui.painter().rect_stroke(r, CornerRadius::same(rad), Stroke::new(1.0_f32, theme::BTNBD), StrokeKind::Inside);
}

/// The background / interaction of one capsule half (`corners` = its rounded outer side).
fn half_bg(ui: &mut Ui, rect: Rect, corners: CornerRadius, salt: &str, enabled: bool, primary: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::new(salt), Sense::click());
    theme::focus_radius(ui.ctx(), resp.id, corners);
    let p = ui.painter();
    if enabled {
        if primary {
            p.rect_filled(rect, corners, theme::ACCENT);
        }
        if resp.hovered() {
            p.rect_filled(rect, corners, theme::HOV);
        }
        // Hover / press outline (the ACCENT one would vanish on the ACCENT fill).
        if resp.is_pointer_button_down_on() {
            p.rect_stroke(rect, corners, Stroke::new(1.0_f32, theme::SELBD), StrokeKind::Inside);
        } else if resp.hovered() && !primary {
            p.rect_stroke(rect, corners, Stroke::new(1.0_f32, theme::ACCENT), StrokeKind::Inside);
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Canvas `.cap2 button` content: 14 px icon, 6 px gap, text, [6 px gap, badge] — centred in `rect`.
fn icon_label(ui: &Ui, rect: Rect, icon: &str, text: &str, font: egui::FontId, col: Color32, badge: Option<(String, Color32, Color32)>) {
    let p = ui.painter();
    let tw = text_w(ui, text, font.clone());
    let bw = badge.as_ref().map(|(t, ..)| badge_w(ui, t));
    let total = 14.0 + 6.0 + tw + bw.map_or(0.0, |b| 6.0 + b);
    let x0 = rect.center().x - total / 2.0;
    let cy = rect.center().y;
    theme::paint_glyph(p, Pos2::new(x0 + 7.0, cy), icon, 13.0, col);
    p.text(Pos2::new(x0 + 20.0, cy), Align2::LEFT_CENTER, text, font, col);
    if let (Some((t, fill, tcol)), Some(bw)) = (badge, bw) {
        let b = Rect::from_center_size(Pos2::new(x0 + 20.0 + tw + 6.0 + bw / 2.0, cy), vec2(bw, 18.0));
        p.rect_filled(b, CornerRadius::same(9), fill);
        p.text(b.center(), Align2::CENTER_CENTER, t, theme::medium(11.0), tcol);
    }
}

/// Canvas `.round`: a 30 px circle, BTN fill, BTNBD outline, DIM icon; SEL / SELBD / ACCENT icon
/// while Settings is the current page.
fn gear(ui: &mut Ui, rect: Rect, on: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::new("topbar_gear"), Sense::click());
    let round = CornerRadius::same(15);
    theme::focus_radius(ui.ctx(), resp.id, round);
    let p = ui.painter();
    p.rect_filled(rect, round, theme::BTN);
    let (border, col) = if on {
        p.rect_filled(rect, round, theme::SEL);
        (theme::SELBD, theme::ACCENT)
    } else {
        if resp.hovered() {
            p.rect_filled(rect, round, theme::HOV);
        }
        (if resp.is_pointer_button_down_on() { theme::SELBD } else if resp.hovered() { theme::ACCENT } else { theme::BTNBD }, theme::DIM)
    };
    p.rect_stroke(rect, round, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    theme::paint_glyph(p, rect.center(), glyph::GEAR, 15.0, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Settings")
}

// ---- row 2: tabs + Sort ------------------------------------------------------------------------

/// Canvas `.utab`: DIM medium label + count badge; the selected tab is TEXT with an ACCENT badge
/// and a 2 px ACCENT underline that overlaps the bar's hairline. In the Settings view none is
/// selected and a click goes back to the main page. A non-zero Updates count is WARN.
fn tabs_row(ui: &mut Ui, layer: &egui::Painter, p: &Plan, state: &AppState, counts: &[String], actions: &mut Vec<Action>) {
    for (i, f) in Filter::ALL.iter().enumerate() {
        let r = p.tabs[i];
        let hit = Rect::from_min_max(Pos2::new(r.left(), r.top() + 2.0), Pos2::new(r.right(), r.bottom() - 3.0));
        let resp = ui.interact(hit, Id::new(("topbar_tab", i)), Sense::click());
        let selected = state.view == View::Main && state.filter == *f;
        let label_col = if selected || resp.hovered() { theme::TEXT } else { theme::DIM };
        let pad_l = if i == 0 { 0.0 } else { 2.0 };
        let cy = r.top() + 17.5;
        let galley = ui.painter().layout_no_wrap(f.label().to_owned(), theme::medium(13.0), label_col);
        let lw = galley.size().x;
        let pos = Pos2::new(r.left() + pad_l, cy - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, label_col);
        let bw = badge_w(ui, &counts[i]);
        let b = Rect::from_min_size(Pos2::new(r.left() + pad_l + lw.ceil() + 6.0, cy - 9.0), vec2(bw, 18.0));
        let (fill, mut col) = if selected { (theme::SEL, theme::ACCENT_T) } else { (theme::BTN, theme::DIM) };
        if *f == Filter::Updates && state.counts().updates > 0 {
            col = theme::WARN;
        }
        ui.painter().rect_filled(b, CornerRadius::same(9), fill);
        ui.painter().text(b.center(), Align2::CENTER_CENTER, &counts[i], theme::medium(11.0), col);
        if selected {
            layer.rect_filled(Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 1.0), Pos2::new(r.right(), r.bottom() + HAIRLINE)), 0.0, theme::ACCENT);
        }
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            actions.push(Action::SetFilter(*f));
            if state.view == View::Settings {
                actions.push(Action::ShowMain);
            }
        }
    }
}

/// Canvas `.btn.ghost.sortb`: arrows icon, "Sort" (DIM), the current choice (TEXT, medium), a
/// chevron — or the icon alone in the narrow form. Opens the sort choices -> `Action::SetSort`.
fn sort_button(ui: &mut Ui, p: &Plan, state: &AppState, actions: &mut Vec<Action>) {
    let r = p.sort;
    let resp = ui.interact(r, Id::new("topbar_sort"), Sense::click());
    theme::focus_radius(ui.ctx(), resp.id, CornerRadius::same(theme::RADIUS));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let g = ui.painter();
    if resp.hovered() || open {
        g.rect_filled(r, CornerRadius::same(theme::RADIUS), theme::BTN);
    }
    let cy = r.center().y;
    if p.sort_icon_only {
        theme::paint_sort_icon(g, r.center(), 15.0, theme::DIM);
    } else {
        let mut x = r.left() + SORT_PAD;
        theme::paint_sort_icon(g, Pos2::new(x + 7.0, cy), 14.0, theme::DIM);
        x += 14.0 + 6.0;
        let lab = g.text(Pos2::new(x, cy), Align2::LEFT_CENTER, "Sort", theme::regular(13.0), theme::DIM);
        x = lab.right() + 6.0;
        g.text(Pos2::new(x, cy), Align2::LEFT_CENTER, state.sort.label(), theme::medium(13.0), theme::TEXT);
        theme::paint_glyph(g, Pos2::new(r.right() - SORT_PAD - 6.0, cy), glyph::CHEVRON, 9.5, theme::DIM);
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let resp = if p.sort_icon_only { resp.on_hover_text(format!("Sort: {}", state.sort.label())) } else { resp };
    let frame = egui::Frame::new()
        .fill(theme::FIELD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(4)
        .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: theme::with_alpha(theme::HEAD, 0.72) });
    egui::Popup::menu(&resp).align(egui::RectAlign::BOTTOM_END).gap(4.0).frame(frame).show(|ui| {
        ui.set_width(170.0);
        ui.spacing_mut().item_spacing.y = 1.0;
        for s in Sort::ALL {
            let (row, rr) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            let g = ui.painter();
            if rr.hovered() {
                g.rect_filled(row, CornerRadius::same(theme::RADIUS), theme::HOV);
            }
            if s == state.sort {
                theme::paint_glyph(g, Pos2::new(row.left() + 14.0, row.center().y), glyph::CHECK, 10.0, theme::ACCENT);
            }
            g.text(Pos2::new(row.left() + 30.0, row.center().y), Align2::LEFT_CENTER, s.label(), theme::regular(13.0), theme::TEXT);
            if rr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if s != state.sort {
                    actions.push(Action::SetSort(s));
                }
                ui.close();
            }
        }
    });
}

// ---- row 1: the search pill ---------------------------------------------------------------------

const HINT: &str = "Search programs";

/// Canvas `.search`: a 30 px pill, FIELD fill, BTNBD border (ACCENT on hover / focus), SEARCH glyph,
/// borderless input, hint and typed text centred (the glyph travels with them as one group). The
/// edit buffer lives in egui memory; it follows `state.search` whenever the state changes behind
/// our back.
fn search_box(ui: &mut Ui, rect: Rect, state: &AppState, actions: &mut Vec<Action>) {
    const PAD: f32 = 16.0;
    let id = Id::new("topbar_search");
    // (buffer, last value we pushed)
    let (mut buf, last): (String, String) =
        ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| (state.search.clone(), state.search.clone()));
    if state.search != last {
        buf = state.search.clone();
    }
    // Background first (the input paints over it); focus is read from memory.
    let hovered = ui.rect_contains_pointer(rect);
    let focused = ui.memory(|m| m.has_focus(id.with("edit")));
    let border = if hovered || focused { theme::ACCENT } else { theme::BTNBD };
    ui.painter().rect(rect, CornerRadius::same((rect.height() / 2.0) as u8), theme::FIELD, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    let inner = Rect::from_min_max(Pos2::new(rect.left() + PAD + 14.0 + 7.0, rect.top()), Pos2::new(rect.right() - PAD, rect.bottom()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let out = egui::TextEdit::singleline(&mut buf)
        .id(id.with("edit"))
        .frame(false)
        .font(theme::regular(13.0))
        .hint_text(theme::placeholder(HINT))
        .horizontal_align(egui::Align::Center)
        .margin(egui::Margin::ZERO)
        .desired_width(inner.width())
        .show(&mut child);
    // The glyph travels with the (centred) hint or text as one group, never past the left padding.
    // `inner` starts one glyph + gap right of the padding, so its centre is where the group's
    // text centre must be for the glyph + text to be centred in the field.
    let text_left = if buf.is_empty() {
        let hint_w = ui.painter().layout_no_wrap(HINT.to_owned(), theme::regular(13.0), theme::DIM).size().x;
        inner.center().x - hint_w * 0.5
    } else {
        out.galley_pos.x
    };
    let glyph_x = (text_left - 7.0 - 7.0).max(rect.left() + PAD + 7.0);
    theme::paint_glyph(ui.painter(), Pos2::new(glyph_x, rect.center().y), glyph::SEARCH, 14.0, theme::DIM);
    let mut sent = last;
    if out.response.changed() {
        actions.push(Action::SetSearch(buf.clone()));
        sent = buf.clone();
    }
    ui.data_mut(|d| d.insert_temp(id, (buf, sent)));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    Secondary,
    Primary,
}

/// Mockup `.btn` / `.btn.primary`, painted by hand so the disabled look matches the mockup
/// (BTN fill, BORDER stroke, FAINT text, even for the primary).
fn button(ui: &mut Ui, rect: Rect, salt: &str, text: &str, kind: Btn, enabled: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::new(salt), Sense::click());
    let p = ui.painter();
    let round = CornerRadius::same(theme::RADIUS);
    let (fill, border, text_col, font) = if !enabled {
        (theme::BTN, theme::BORDER, theme::FAINT, theme::regular(13.0))
    } else {
        let pressed = resp.is_pointer_button_down_on();
        match kind {
            Btn::Primary => (theme::ACCENT, if pressed { theme::SELBD } else { theme::ACCENT }, theme::PRIMARY_TEXT, theme::bold(13.0)),
            Btn::Secondary => (
                theme::BTN,
                if pressed { theme::SELBD } else if resp.hovered() { theme::ACCENT } else { theme::BTNBD },
                theme::TEXT,
                theme::regular(13.0),
            ),
        }
    };
    p.rect(rect, round, fill, Stroke::new(1.0_f32, border), StrokeKind::Inside);
    if enabled && resp.hovered() && kind == Btn::Secondary {
        p.rect_filled(rect.shrink(1.0), CornerRadius::same(theme::RADIUS - 1), theme::HOV);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, text_col);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// The "ritz-get-gui was updated. Restart to use the new version." bar (mockup `.restart-bar`:
/// GOOD @12 % fill, GOOD @42 % border, radius 7, padding 4 4 4 10, 7 px GOOD dot + message +
/// primary Restart -> `Action::Restart`). `ui::draw` only calls this when `state.restart`. The
/// button wraps under the message when the row is too narrow.
pub fn draw_restart(ui: &mut Ui, state: &AppState, actions: &mut Vec<Action>) {
    let _ = state;
    let frame = egui::Frame::new()
        .fill(theme::with_alpha(theme::GOOD, 0.12))
        .stroke(Stroke::new(1.0_f32, theme::with_alpha(theme::GOOD, 0.42)))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin { left: 10, right: 4, top: 4, bottom: 4 });
    theme::framed(ui, frame, |ui| {
        let w = ui.available_width().max(0.0);
        const MSG: &str = "ritz-get-gui was updated. Restart to use the new version.";
        const DOT: f32 = 7.0;
        let btn_w = btn_w(ui, "Restart", theme::bold(13.0));
        let msg_x = DOT + 8.0;
        let one_line = ui.painter().layout_no_wrap(MSG.to_owned(), theme::regular(13.0), theme::TEXT);
        let fits = msg_x + one_line.size().x + GAP + btn_w <= w;
        let origin = ui.cursor().min;
        let ctrl_h = theme::CTRL_H;
        let (msg_h, total_h, btn_rect);
        let galley = if fits {
            msg_h = one_line.size().y;
            total_h = ctrl_h;
            btn_rect = Rect::from_min_size(Pos2::new(origin.x + w - btn_w, origin.y), vec2(btn_w, ctrl_h));
            one_line
        } else {
            let g = ui.painter().layout(MSG.to_owned(), theme::regular(13.0), theme::TEXT, (w - msg_x).max(40.0));
            msg_h = g.size().y;
            let row1 = msg_h.max(ctrl_h);
            total_h = row1 + GAP + ctrl_h;
            btn_rect = Rect::from_min_size(Pos2::new(origin.x, origin.y + row1 + GAP), vec2(btn_w, ctrl_h));
            g
        };
        let msg_cy = origin.y + if fits { ctrl_h } else { msg_h.max(ctrl_h) } / 2.0;
        let p = ui.painter();
        p.circle_filled(Pos2::new(origin.x + DOT / 2.0, msg_cy), DOT / 2.0, theme::GOOD);
        p.galley(Pos2::new(origin.x + msg_x, msg_cy - msg_h / 2.0), galley, theme::TEXT);
        ui.advance_cursor_after_rect(Rect::from_min_size(origin, vec2(w, total_h)));
        if button(ui, btn_rect, "topbar_restart", "Restart", Btn::Primary, true).clicked() {
            actions.push(Action::Restart);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plausible measured widths (Geist 13 px): wordmark, halves, tabs, Sort ghost.
    fn metrics(window_w: f32) -> Metrics {
        let w = window_w - 16.0;
        Metrics {
            w,
            centre_x: 8.0 + w / 2.0,
            brand: 126.0,
            check_full: 164.0,
            check_icon: ICON_HALF,
            upd: 134.0,
            tabs: [48.0, 92.0, 86.0, 118.0],
            sort_full: 160.0,
            sort_max: 172.0,
        }
    }

    fn plan_at(window_w: f32) -> (Plan, f32) {
        let m = metrics(window_w);
        (plan(&m, Pos2::new(8.0, 10.0)), 8.0 + m.w)
    }

    #[test]
    fn wide_form_at_1280_and_900() {
        for window_w in [1280.0_f32, 900.0] {
            let (p, right) = plan_at(window_w);
            assert!(!p.narrow, "{window_w} must be the wide form");
            // Row 1: everything on one 30 px row, the search centred in the window.
            for r in [p.search, p.capsule, p.gear, p.brand] {
                assert!(r.top() >= 10.0 - 0.01 && r.bottom() <= 40.0 + 0.01, "{window_w}: {r:?} outside row 1");
            }
            assert!((p.search.center().x - window_w / 2.0).abs() < 0.01, "search centred: {:?}", p.search);
            assert!(p.search.width() >= SEARCH_MIN_CENTRED && p.search.width() <= SEARCH_W);
            assert!(p.search.left() >= p.brand.right() + GAP - 0.01 && p.search.right() <= p.capsule.left() - GAP + 0.01);
            // Capsule | 8 | gear, gear flush right; halves inside the border, 1 px divider.
            assert!((p.gear.right() - right).abs() < 0.01 && (p.capsule.right() + GAP - p.gear.left()).abs() < 0.01);
            assert!((p.upd.left() - p.check.right() - 1.0).abs() < 0.01 && (p.capsule.right() - p.upd.right() - 1.0).abs() < 0.01);
            assert!(!p.sort_icon_only);
            assert_eq!(p.height, ROW1_H + GAP + ROW2_H);
        }
        // Search is the full 460 at 1280 and has shrunk (still centred) at 900.
        assert_eq!(plan_at(1280.0).0.search.width(), SEARCH_W);
        assert!(plan_at(900.0).0.search.width() < SEARCH_W);
    }

    #[test]
    fn narrow_form_at_600_and_520() {
        for window_w in [600.0_f32, 520.0] {
            let (p, right) = plan_at(window_w);
            assert!(p.narrow && p.sort_icon_only, "{window_w} must be the narrow form");
            // Row 1 keeps brand | capsule (icon-only Check) | gear; the search is a full-width row below.
            assert_eq!(p.check.width(), ICON_HALF);
            assert!(p.capsule.left() >= p.brand.right() + GAP - 0.01);
            assert_eq!((p.search.left(), p.search.right()), (8.0, right));
            assert_eq!(p.search.top(), 10.0 + ROW1_H + GAP + 2.0);
            assert_eq!(p.row2.top(), p.search.bottom() + GAP);
            assert_eq!(p.height, ROW1_H + GAP + 2.0 + SEARCH_H + GAP + ROW2_H);
            // Tabs: 16 px apart, none runs into the Sort icon.
            assert!((p.tabs[1].left() - p.tabs[0].right() - TAB_GAP_NARROW).abs() < 0.01);
            assert!(p.tabs[3].right() <= p.sort.left() + 0.01, "tabs reach the sort icon at {window_w}");
        }
    }

    /// The Sort button's arrow ends at the gear's right edge, in both forms.
    #[test]
    fn sort_chevron_aligns_with_the_gear() {
        let (wide, _) = plan_at(1280.0);
        assert!((wide.sort.right() - SORT_PAD - wide.gear.right()).abs() < 0.01, "chevron box right edge vs gear");
        let (narrow, _) = plan_at(600.0);
        let icon_right = narrow.sort.center().x + 15.0 / 2.0;
        assert!((icon_right - narrow.gear.right()).abs() < 0.01, "icon ink right edge vs gear");
        // Tabs: the first is flush under the wordmark; every tab fills the 36 px row.
        for p in [&wide, &narrow] {
            assert_eq!(p.tabs[0].left(), p.brand.left());
            assert!(p.tabs.iter().all(|t| t.height() == ROW2_H && t.top() == p.row2.top()));
        }
    }

    /// The form flips exactly where the search can no longer be centred at >= SEARCH_MIN_CENTRED.
    #[test]
    fn form_flips_at_the_centred_search_minimum() {
        let flip = (600..=900).find(|&w| !plan_at(w as f32).0.narrow).expect("a wide window exists below 900") as f32;
        assert!((750.0..=890.0).contains(&flip), "wide form starts at {flip}");
        assert!(plan_at(flip).0.search.width() >= SEARCH_MIN_CENTRED);
        assert!(plan_at(flip - 1.0).0.narrow);
    }
}
