# Styling Guide

The look is "Graphite", a single dark theme (no light variant). Everything reusable lives in
`src/theme.rs` (tokens, fonts, pills, buttons, pane helpers, `theme::apply`) and `src/ui/mod.rs`
(window frame, `vscroll`). The design mockup the theme follows is
`design/ritz-get-gui-graphite.html` (its `:root` custom properties carry the same role names).

**Top rule: reference colours by role token (`theme::PANEL`, `theme::ACCENT`, ...), never a hex
literal at the call site.** Colours that carry meaning (pill colours, log line kinds) are picked
from the same tokens, never new hex values (`src/theme.rs` module docs).

## Tokens and sizes

Colour roles (`src/theme.rs`): backgrounds `HEAD` (top bar), `PANEL` (main), `PANEL2` (status bar,
side panel), `FIELD` (inputs, code boxes); lines `BORDER`; text `TEXT`, `DIM`, `FAINT`,
`TEXT_DIM`, `TEXT_FAINT`; controls `BTN`, `BTNBD`, `PRIMARY_TEXT`; meaning `ACCENT`, `GOOD`, `WARN`,
`DANGER`; tints `SEL`, `SELBD`, `HOV`. Derive a translucent colour with `theme::with_alpha(c, a)`.

Sizes: `RADIUS` 7, `GAP` 8 (between cards, columns, panes), `ROW_GAP` 4, `CARD_PAD` 6,
`CTRL_H` 22, `MARGIN` 8, `SIDE_W` 300, `STACK_BELOW` 760 (below this window width the side panel
stacks under the groups), `PILL_H` 20.

    use crate::theme;
    let frame = egui::Frame::new()
        .fill(theme::PANEL2)
        .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(theme::MARGIN));

Do NOT write `Color32::from_rgb(...)` or a hex value in a pane or modal. Add or reuse a token so
a re-theme stays one edit. (Known exception: `src/ui/modals.rs` still declares three private
colour consts, `TRACK`, `DANGER_T`, `BAR_DIM`, for the progress bars. Do not copy that pattern.)

## Fonts

Geist for all UI text; Geist Mono only for versions, hashes, paths, the log and code.

    ui.label(egui::RichText::new("Name").font(theme::bold(16.0)).color(theme::TEXT));
    ui.label(egui::RichText::new("2026-10-14 3f9e21a").font(theme::mono(12.0)).color(theme::TEXT_DIM));

Helpers: `theme::regular`, `theme::medium` (pills), `theme::bold` (names, titles, primary button),
`theme::mono`. Icon glyphs come from `theme::glyph::*` (bundled Nerd font via the fallback chain);
paint them with `theme::paint_glyph` or `theme::icon_button`, which centre on the glyph's ink.

That centring is only as exact as the pixel grid: `theme::centered_pos` centres the glyph's whole-pixel
box, and the tessellator then rounds the position to a whole pixel, half-pixels going right and down.
So a glyph box of odd pixel size in an even-sized control sits about 0.5 px off, and the box can
differ from the real ink by a further fraction. If an icon looks off-centre, pick a font size whose
box parity matches the control, as `src/ui/topbar.rs:gear` does (14, not 15, in the 30 px circle).
*Why:* painting at a fractional position would need a hand-built mesh, and the linear-filtered font
atlas would blur the icon. The parity holds at scale 1 and 2; fractional scales can stay about 0.5 pt off.

Do NOT use Geist Mono for prose or labels. It is for aligned or literal text only, so the UI keeps
one voice.

## Panes

Every region is its own clipped pane, and a pane module only sizes its content inside the pane it
is handed (`src/ui/mod.rs` module docs). Size rows from `ui.available_width()`; use
`theme::columns_sized`, `theme::framed`, `theme::card` rather than free-floating layouts, so an
over-wide row is cut at the pane edge instead of widening or painting into a neighbour.

    theme::card(ui, "Program", |ui| {
        // rows sized from ui.available_width()
    });

A pane has the signature `pub fn draw(ui: &mut egui::Ui, state: &AppState, actions: &mut Vec<Action>)`.

Do NOT mutate `AppState` from a pane. Panes get `&AppState` and push `Action`s; `App::dispatch`
applies them after drawing (`src/ui/mod.rs`, `src/model.rs:Action`).

## Buttons

Three variants, all in `src/theme.rs`: `primary_button` (solid accent, one main action),
`danger_button` (transparent, red text and border; destructive), `secondary_button` (neutral).

    let go = ui.add_enabled(state.actions_enabled(), theme::danger_button("Discard and reset"));
    theme::hover_outline(ui, &go, theme::DANGER); // solid red hover border for the danger variant
    if go.clicked() {
        actions.push(Action::Reset { name });
    }

For a fixed size, chain egui's `min_size` on the returned button, as `src/ui/modals.rs:mbtn` does.
A square glyph button is `theme::icon_button(ui, theme::glyph::GEAR, 22.0)`.

Do NOT hand-roll buttons (`egui::Button::new(...).fill(...)`). Use `primary_button`,
`danger_button` or `secondary_button`, so the fill, border, hover and disabled looks stay uniform.
Disable action controls unless `state.actions_enabled()`.

## Pills

A pill is the small uppercase status label on a card. The mapping from `(state, entry)` to pills
is one pure function, `src/ui/pills.rs:pills`; paint with `pills::show`, size with `pills::width`.
Colours come from `src/theme.rs:PillKind` (GOOD up to date; WARN update, modified, branch deleted,
setup needed; DANGER fetch failed, needs tool; ACCENT website, setup; TEXT_DIM not installed,
offline, held). A glyph-only pill is the held pause mark.

    for p in pills::pills(state, entry) {
        pills::show(ui, &p);
    }

Do NOT build a pill by hand in a pane. Add a `PillKind` and a row in `pills::pills` (and its table),
so cards and the side hero never disagree.

## Scrolling

Every vertical scroll area comes from `ui::vscroll(touch)`, which makes drag-to-scroll follow
touch mode (wheel and scrollbar always work). A scrolling card grid also calls
`theme::solid_scroll_style` for the solid 6 px bar.

    crate::ui::vscroll(state.touch_mode)
        .id_salt("my_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| { /* content */ });

Do NOT call `egui::ScrollArea::vertical()` directly. It would ignore touch mode. (Known leftover:
one raw `ScrollArea` in `src/ui/modals.rs`, the Delete-data path list; do not copy it.)
Panes such as `groups` and `settings` must not create their own scroll area: `ui::draw_panes`
wraps them.

## Layout breakpoint

At window width of 760 px or more the body is `[groups | 8 | side 300]` with independent scrolling;
below it, one scroll area holds groups then the side panel (`src/ui/mod.rs:draw_panes`). The
constants are `theme::STACK_BELOW`, `theme::SIDE_W`. Test layout changes with the overflow tests in
`src/ui/test_render.rs`.
