//! The window frame (**I1, final**): `VBox[TopBar, RestartBar, Banner, Body, StatusBar]` with
//! `Body = HBox[Groups | Side(300)]` at window width >= 760 and `VBox[Groups, Side]` below it
//! (breakpoint from `design/ritz-get-gui-graphite.html`), following the Graphite "nested panes" rule: every region is its
//! own clipped pane, and each pane module only sizes its content inside the pane it is handed.
//!
//! Pane modules and owners (each exposes `pub fn draw...(ui, &AppState, &mut Vec<Action>)`; none
//! may mutate state, they only push [`Action`]s):
//!
//! | module     | entry points                                  | pane it is handed (what `mod.rs` already drew) | plan |
//! |------------|-----------------------------------------------|-------------------------------------------------|------|
//! | `topbar`   | `draw`, `draw_restart`                        | HEAD panel, margins L/R 8, T 10, B 1 = the hairline (`draw`); PANEL strip with margins L/R/T 8 (`draw_restart`, which draws the bar itself) | I8 |
//! | `groups`   | `draw`                                        | a clipped, full-width pane inside a vertical `ScrollArea`; no frame | I9 |
//! | `side`     | `draw`                                        | PANEL2 frame, BORDER stroke, radius 7, inner margin 8, inside a vertical `ScrollArea` | I10 |
//! | `bottom`   | `draw_status`                                 | PANEL2 strip, margins L/R 8, T/B 4, top hairline already drawn (the log drawer is gone: see `modals` / `logview`) | I11 |
//! | `settings` | `draw`                                        | the whole body (replaces groups+side when `view == Settings`), inside a vertical `ScrollArea` | I12 |
//! | `modals`   | `draw` (overlay), `draw_banner`               | `draw`: the `Context` (egui `Modal`s); `draw_banner`: PANEL strip with margins L/R/T 8 | I13 |
//!
//! Each pane also gets `item_spacing = (8, 8)` and the Graphite style from [`crate::theme::apply`].

use egui::{Frame, Margin, Rect, Stroke};

use crate::app::AppState;
use crate::model::{Action, View};
use crate::theme;

pub mod bottom;
pub mod groups;
pub mod logview;
pub mod modals;
pub mod pills;
pub mod settings;
pub mod side;
pub mod topbar;

#[cfg(test)]
mod test_render;

/// Where each pane ended up this frame (used by the pane-overflow test; also handy for debugging).
/// In the stacked layout `groups`/`side` are in scrolled content coordinates (they can extend
/// below the window); in the Settings view `groups` is the whole body and `side` is `None`.
// Read by the pane-overflow tests only; the app itself discards the rects.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub struct PaneRects {
    pub top: Rect,
    pub restart: Option<Rect>,
    pub banner: Option<Rect>,
    pub status: Rect,
    /// The CentralPanel (its background fill legitimately spans groups + side).
    pub central: Rect,
    pub groups: Rect,
    pub side: Option<Rect>,
    /// `side` sits under `groups` (window narrower than [`theme::STACK_BELOW`]).
    pub stacked: bool,
}

/// Clip a panel's content to its inner width (4 px slack for focus rings / hover washes), so an
/// over-wide row is cut at the pane edge instead of painting into the panel's margin.
/// The app's only way to make a vertical `ScrollArea`: dragging with a held mouse button scrolls
/// only in touch mode (`state.touch_mode`); wheel, touchpad and scrollbar always work.
pub fn vscroll(touch: bool) -> egui::ScrollArea {
    egui::ScrollArea::vertical().scroll_source(scroll_source(touch))
}

/// Touch mode on = drag the contents to scroll; off = never. Wheel and scrollbar stay on.
pub fn scroll_source(touch: bool) -> egui::scroll_area::ScrollSource {
    egui::scroll_area::ScrollSource { drag: touch, ..egui::scroll_area::ScrollSource::default() }
}

fn clip_x(ui: &mut egui::Ui) {
    clip_x_by(ui, 4.0);
}

fn clip_x_by(ui: &mut egui::Ui, slack: f32) {
    let r = ui.max_rect();
    ui.shrink_clip_rect(Rect::from_x_y_ranges(r.x_range().expand(slack), ui.clip_rect().y_range()));
}

fn bordered(fill: egui::Color32, margin: i8) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::same(margin))
}

/// Draw the whole window: the panes, then the modal overlays. Call once per frame from `App::update`.
pub fn draw(ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) -> PaneRects {
    let rects = draw_panes(ctx, state, actions);
    modals::draw(ctx, state, actions);
    rects
}

/// Everything but the modal overlays (split out so the pane-overflow test can check the panes on
/// their own and the overlays separately).
pub fn draw_panes(ctx: &egui::Context, state: &AppState, actions: &mut Vec<Action>) -> PaneRects {
    let m = theme::MARGIN;

    // ---- top: bar, restart bar, ritz-get-not-found banner ----
    let top = egui::TopBottomPanel::top("topbar")
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::HEAD).inner_margin(Margin { left: m, right: m, top: topbar::PAD_TOP, bottom: topbar::HAIRLINE as i8 }))
        .show(ctx, |ui| {
            clip_x_by(ui, topbar::CLIP_SLACK);
            topbar::draw(ui, state, actions)
        })
        .response
        .rect;

    let strip = || Frame::new().fill(theme::PANEL).inner_margin(Margin { left: m, right: m, top: m, bottom: 0 });
    let restart = state.restart.then(|| {
        egui::TopBottomPanel::top("restartbar")
            .show_separator_line(false)
            .frame(strip())
            .show(ctx, |ui| {
                clip_x(ui);
                topbar::draw_restart(ui, state, actions)
            })
            .response
            .rect
    });
    let banner = state.ritz_get_missing.then(|| {
        egui::TopBottomPanel::top("banner")
            .show_separator_line(false)
            .frame(strip())
            .show(ctx, |ui| {
                clip_x(ui);
                modals::draw_banner(ui, state, actions)
            })
            .response
            .rect
    });

    // ---- bottom: status bar ----
    let status = egui::TopBottomPanel::bottom("status")
        .show_separator_line(false)
        .frame(Frame::new().fill(theme::PANEL2).inner_margin(Margin { left: m, right: m, top: 4, bottom: 4 }))
        .show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range().expand(m as f32), r.top() - 4.0, Stroke::new(1.0_f32, theme::BORDER));
            clip_x(ui);
            ui.spacing_mut().item_spacing = egui::vec2(theme::GAP, 4.0);
            bottom::draw_status(ui, state, actions);
        })
        .response
        .rect;

    // ---- body ----
    let mut groups = Rect::NOTHING;
    let mut side_rect = None;
    let mut stacked = false;
    let central = egui::CentralPanel::default()
        .frame(Frame::new().fill(theme::PANEL).inner_margin(Margin::same(m)))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(theme::GAP, theme::GAP);
            match state.view {
                View::Settings => groups = body_settings(ui, state, actions),
                View::Main => {
                    stacked = ui.available_width() < theme::STACK_BELOW;
                    (groups, side_rect) = if stacked {
                        body_stacked(ui, state, actions)
                    } else {
                        body_side_by_side(ui, state, actions)
                    };
                }
            }
        })
        .response
        .rect;

    PaneRects { top, restart, banner, status, central, groups, side: side_rect, stacked }
}

/// Settings replaces groups + side: one full-width pane in a vertical scroll area.
fn body_settings(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) -> Rect {
    let w = ui.available_width();
    let slot = Rect::from_min_max(ui.cursor().min, egui::pos2(ui.cursor().min.x + w, ui.max_rect().bottom()));
    theme::columns_sized(ui, &[w], 0.0, |c| {
        vscroll(state.touch_mode)
            .id_salt("settings_scroll")
            .auto_shrink([false, false])
            .show(&mut c[0], |ui| settings::draw(ui, state, actions));
    });
    slot
}

/// Window >= 760 px: `[ groups (fills) | 8 | side (300, full height) ]`, each pane scrolls on its own.
fn body_side_by_side(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) -> (Rect, Option<Rect>) {
    let total = ui.available_width();
    let groups_w = (total - theme::SIDE_W - theme::GAP).max(0.0);
    // The slots, not the (possibly grown) child rects: what the pane test measures against.
    let (o, bottom) = (ui.cursor().min, ui.max_rect().bottom());
    let groups_slot = Rect::from_min_max(o, egui::pos2(o.x + groups_w, bottom));
    let side_slot = Rect::from_min_max(egui::pos2(groups_slot.right() + theme::GAP, o.y), egui::pos2(groups_slot.right() + theme::GAP + theme::SIDE_W, bottom));
    theme::columns_sized(ui, &[groups_w, theme::SIDE_W], theme::GAP, |c| {
        let [g, s] = c else { unreachable!("two columns") };
        let orig = theme::solid_scroll_style(g);
        vscroll(state.touch_mode)
            .id_salt("groups_scroll")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(g, |ui| {
                *ui.visuals_mut() = orig;
                groups::draw(ui, state, actions)
            });

        // The side pane stretches to the bottom of the body (mockup `.side`), so draw its
        // background over the whole column, then its content in a scroll area inside it.
        let rect = side_slot;
        s.painter().rect(
            rect,
            egui::CornerRadius::same(theme::RADIUS),
            theme::PANEL2,
            Stroke::new(1.0_f32, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(1.0 + theme::GAP);
        let mut child = s.new_child(
            egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down_justified(egui::Align::LEFT)),
        );
        // Clip just inside the stroke (like `theme::framed`): content never reaches the border.
        child.set_clip_rect(rect.shrink2(egui::vec2(2.0, 1.0)).intersect(child.clip_rect()));
        vscroll(state.touch_mode)
            .id_salt("side_scroll")
            .auto_shrink([false, false])
            .show(&mut child, |ui| side::draw(ui, state, actions));
    });
    (groups_slot, Some(side_slot))
}

/// Window < 760 px: one vertical scroll area holding groups (natural height), 8 px, then the side
/// panel (natural height).
fn body_stacked(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>) -> (Rect, Option<Rect>) {
    let orig = theme::solid_scroll_style(ui);
    vscroll(state.touch_mode)
        .id_salt("body_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            *ui.visuals_mut() = orig;
            let w = ui.available_width();
            let top_left = ui.cursor().min;
            let groups_h = theme::columns_sized(ui, &[w], 0.0, |c| {
                groups::draw(&mut c[0], state, actions);
                c[0].min_rect().height()
            });
            let groups = Rect::from_min_size(top_left, egui::vec2(w, groups_h));
            if state.listing.sections.is_empty() {
                // No programs (ritz-get missing / refused / first `list` pending): the side panel
                // would be an empty sliver, so there is none in this layout.
                return (groups, None);
            }
            ui.add_space(theme::GAP);
            let y0 = ui.cursor().top();
            let x0 = ui.cursor().left();
            theme::framed(ui, bordered(theme::PANEL2, theme::GAP as i8), |ui| side::draw(ui, state, actions));
            let side = Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x0 + w, ui.cursor().top()));
            if state.scroll_to_side {
                // A card was just selected: its side panel is below the fold in this layout.
                ui.scroll_to_rect(side, Some(egui::Align::TOP));
            }
            (groups, Some(side))
        })
        .inner
}

#[cfg(test)]
mod scroll_tests {
    #[test]
    fn scroll_config_follows_touch_mode() {
        let off = super::scroll_source(false);
        assert!(!off.drag);
        assert!(off.mouse_wheel && off.scroll_bar, "wheel and scrollbar always work");
        let on = super::scroll_source(true);
        assert!(on.drag && on.mouse_wheel && on.scroll_bar);
    }
}
