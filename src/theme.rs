//! "Graphite" theme — role-based colour tokens + egui style setup.
//! One place re-themes the whole app (egui 0.33). Reference colours by *role* (PANEL, ACCENT, DIM…), never hard-code hex at call sites.
//!
//! Colours that carry meaning in the data (pill status colours, log line kinds) are picked from
//! these tokens at the call site / in [`PillKind`] — never new hex values.
//!
//! **Font rule (D40):** Geist for all UI text ([`FontId::proportional`], [`medium`], [`bold`]);
//! Geist Mono ([`mono`]) only for versions/hashes, paths, the log and code.
//! Icon glyphs ([`glyph`]) come from the bundled Nerd font through the fallback chain.

use egui::{Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, TextStyle};

// ---- Brand / chrome ------------------------------------------------------

/// Brand accent (indigo). Selected tabs, primary action, section labels.
pub const ACCENT: Color32 = Color32::from_rgb(0x5B, 0x8B, 0xF0);
/// Lighter ACCENT for text on a SEL wash (canvas `--accent-t`: the selected tab's count badge).
pub const ACCENT_T: Color32 = Color32::from_rgb(0x86, 0xA9, 0xF5);
/// Title-bar / top-panel background (darker than panels).
pub const HEAD: Color32 = Color32::from_rgb(0x14, 0x17, 0x1A);
/// Main panel / column background.
pub const PANEL: Color32 = Color32::from_rgb(0x1E, 0x21, 0x25);
/// Footer / status band background.
pub const PANEL2: Color32 = Color32::from_rgb(0x19, 0x1C, 0x1F);
/// All hairline borders / dividers.
pub const BORDER: Color32 = Color32::from_rgb(0x2C, 0x30, 0x36);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xE7, 0xE9, 0xEC);
/// Secondary text.
pub const DIM: Color32 = Color32::from_rgb(0x96, 0x9C, 0xA6);
/// Tertiary text / labels / placeholders.
pub const FAINT: Color32 = Color32::from_rgb(0x64, 0x6A, 0x73);
/// Input & code-block background.
pub const FIELD: Color32 = Color32::from_rgb(0x15, 0x18, 0x1B);
/// Secondary button background.
pub const BTN: Color32 = Color32::from_rgb(0x26, 0x2A, 0x30);
/// Button border.
pub const BTNBD: Color32 = Color32::from_rgb(0x34, 0x39, 0x41);
/// Text on a primary (accent) button.
pub const PRIMARY_TEXT: Color32 = Color32::from_rgb(0x0B, 0x12, 0x22);
/// Destructive / danger (delete, stop, disconnect).
pub const DANGER: Color32 = Color32::from_rgb(0xE1, 0x55, 0x54);
/// Positive / connected (green).
pub const GOOD: Color32 = Color32::from_rgb(0x6C, 0xC5, 0x51);
/// Text of a disabled control on a BTN / HEAD fill (canvas `--off`).
pub const OFF: Color32 = Color32::from_rgb(0x7C, 0x83, 0x8D);
/// Caution / in-between state (muted pastel amber — e.g. gearbox not yet calibrated).
pub const WARN: Color32 = Color32::from_rgb(0xD8, 0xB4, 0x55);

// ---- Steel scale (dashboard neutrals) -------------------------------------
// Widget chrome uses these blue-tinted neutrals instead of pure grays so the
// dashboard leans toward the indigo accent. `lum` tracks the `from_gray` value
// it replaces (green channel = lum keeps perceived brightness ~equal).

/// Blue-tinted neutral: r ≈ 0.9·lum, g = lum, b ≈ 1.17·lum (saturating).
pub const fn steel(lum: u8) -> Color32 {
    let b = lum as u16 + lum as u16 / 6;
    let b = if b > 255 { 255 } else { b };
    Color32::from_rgb(lum - lum / 10, lum, b as u8)
}

/// Secondary widget text: unit labels, legends, muted values (was gray ~140–160).
pub const TEXT_DIM: Color32 = steel(150);
/// Faint hint / caption text: placeholders, sub-labels (was gray ~90–120).
pub const TEXT_FAINT: Color32 = steel(105);

// Derived selection / hover tints (premultiplied — const-friendly).
pub const SEL: Color32 = Color32::from_rgba_premultiplied(0x0F, 0x16, 0x27, 0x29);
pub const SELBD: Color32 = Color32::from_rgba_premultiplied(0x26, 0x3A, 0x65, 0x6B);
pub const HOV: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);

/// `c` with an explicit alpha `0.0..=1.0` (unmultiplied), e.g. the pill recipe: fill `c`@16 %,
/// border `c`@42 %.
pub fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

// ---- Metrics ("Type & metrics") ------------------------------------

/// Corner radius of widgets and cards.
pub const RADIUS: u8 = 7;
/// Gap between cards / columns / panes.
pub const GAP: f32 = 8.0;
/// Gap between rows inside a card.
pub const ROW_GAP: f32 = 4.0;
/// Card inner margin.
pub const CARD_PAD: i8 = 6;
/// Standard control height.
pub const CTRL_H: f32 = 22.0;
/// Window / content outer margin.
pub const MARGIN: i8 = 8;
/// Below this window width the side panel stacks under the groups (mockup breakpoint).
pub const STACK_BELOW: f32 = 760.0;
/// Width of the side panel when it sits beside the groups.
pub const SIDE_W: f32 = 300.0;

// ---- Fonts ---------------------------------------------------------------
//
// egui has no font weights, so each weight is its own named family. All of them fall back to the
// Nerd font so icon glyphs (`\uf…`) render in any weight.

/// `FontFamily::Name` of Geist Medium (weight 500: pills).
pub const FAMILY_MEDIUM: &str = "medium";
/// `FontFamily::Name` of Geist Bold (weight 700: names, titles, wordmark, primary button).
pub const FAMILY_BOLD: &str = "bold";

/// Geist Medium at `size`.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_MEDIUM.into()))
}
/// Geist Bold at `size`.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_BOLD.into()))
}
/// Geist Regular at `size` (the Proportional family).
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
/// Geist Mono Regular at `size` (the Monospace family). Versions, hashes, paths, log, code.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Bundle the Geist fonts (D40) and set up the families:
/// Proportional = Geist -> Nerd; Monospace = Geist Mono -> Nerd; `medium` / `bold`
/// = their weight -> Geist (Geist Mono) -> Nerd. egui's built-in fonts stay at the tail as
/// coverage fallbacks. Call once at startup (and in tests, on a fresh `Context`).
pub fn install_fonts(ctx: &egui::Context) {
    use egui::FontData;
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), FontData::from_static(bytes).into());
    };
    add("geist", include_bytes!("../assets/fonts/Geist-Regular.ttf"));
    add("geist-medium", include_bytes!("../assets/fonts/Geist-Medium.ttf"));
    add("geist-bold", include_bytes!("../assets/fonts/Geist-Bold.ttf"));
    add("geist-mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf"));
    add("nerd", include_bytes!("../assets/fonts/GeistMonoNerdFont-Regular.otf"));

    let s = |x: &str| x.to_owned();
    // Text from the clean TTFs, icon codepoints from the patched OTF.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.splice(0..0, [s("geist"), s("nerd")]);
    let monof = fonts.families.entry(FontFamily::Monospace).or_default();
    monof.splice(0..0, [s("geist-mono"), s("nerd")]);
    for (family, chain) in [
        (FAMILY_MEDIUM, ["geist-medium", "geist", "nerd"]),
        (FAMILY_BOLD, ["geist-bold", "geist", "nerd"]),
    ] {
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain.iter().map(|x| s(x)).collect());
    }
    ctx.set_fonts(fonts);
}

// ---- Icon glyphs ---------------------------------------------------------

/// The 20 Nerd Font (Font Awesome range) glyphs of the mockup, as `&str` so they drop straight
/// into a label / button. All verified present in `GeistMonoNerdFont-Regular.otf`.
/// Draw them centred on their ink with [`paint_glyph`] / [`icon_button`], not their advance box.
pub mod glyph {
    pub const GEAR: &str = "\u{F013}";
    pub const GITHUB: &str = "\u{F09B}";
    /// external link `↗`-style box
    pub const EXT: &str = "\u{F08E}";
    pub const DOWNLOAD: &str = "\u{F019}";
    pub const REFRESH: &str = "\u{F021}";
    pub const TRASH: &str = "\u{F1F8}";
    pub const UNDO: &str = "\u{F0E2}";
    pub const FOLDER: &str = "\u{F07B}";
    pub const PLUS: &str = "\u{F067}";
    pub const MINUS: &str = "\u{F068}";
    pub const BOOK: &str = "\u{F02D}";
    pub const BUG: &str = "\u{F188}";
    pub const CHECK: &str = "\u{F00C}";
    pub const PAUSE: &str = "\u{F04C}";
    pub const SEARCH: &str = "\u{F002}";
    /// chevron pointing down (expanded); rotate/swap for collapsed
    pub const CHEVRON: &str = "\u{F078}";
    /// chevron pointing right (collapsed)
    pub const CHEVRON_R: &str = "\u{F054}";
    pub const TERMINAL: &str = "\u{F120}";
    pub const WARNING: &str = "\u{F071}";
}

thread_local! {
    /// Ink centre (relative to the galley's top-left) per (first char, font size, ppp). The fonts are
    /// built once, so the first measurement of a glyph at a size stays right; ppp is in the key
    /// because the mesh snaps to device pixels.
    static INK_CENTER: std::cell::RefCell<std::collections::HashMap<(char, u32, u32), egui::Vec2>> = Default::default();
}

/// Where to paint `glyph` with `Align2::LEFT_TOP` so the centre of the pixels it actually draws (the
/// galley's `mesh_bounds`, not its advance / line box) lands on `cell_center`. Measured once per
/// (glyph, size, ppp); a glyph that draws nothing falls back to the layout-box centre.
pub fn centered_pos(painter: &egui::Painter, cell_center: egui::Pos2, glyph: &str, font: &FontId) -> egui::Pos2 {
    let Some(ch) = glyph.chars().next() else { return cell_center };
    let key = (ch, font.size.to_bits(), painter.ctx().pixels_per_point().to_bits());
    let ink = INK_CENTER.with(|c| {
        *c.borrow_mut().entry(key).or_insert_with(|| {
            let g = painter.layout_no_wrap(ch.to_string(), font.clone(), Color32::WHITE);
            if g.mesh_bounds.is_positive() { g.mesh_bounds.center().to_vec2() } else { g.size() * 0.5 }
        })
    });
    cell_center - ink
}

/// Paint `glyph` centred on its **ink** at `center` (Nerd glyphs sit off-centre in their advance box).
pub fn paint_glyph(painter: &egui::Painter, center: egui::Pos2, glyph: &str, size: f32, color: Color32) {
    let font = FontId::proportional(size);
    painter.text(centered_pos(painter, center, glyph, &font), egui::Align2::LEFT_TOP, glyph, font, color);
}

/// The "sort" icon (two vertical arrows, canvas `arrow-up-down`) centred at `center`, `size` px square.
pub fn paint_sort_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * k, (y - 12.0) * k);
    let st = Stroke::new((2.0 * k).max(1.0), color);
    // down arrow on the right (x = 17), up arrow on the left (x = 7)
    painter.line_segment([pt(17.0, 4.0), pt(17.0, 20.0)], st);
    painter.line_segment([pt(7.0, 4.0), pt(7.0, 20.0)], st);
    painter.line_segment([pt(21.0, 16.0), pt(17.0, 20.0)], st);
    painter.line_segment([pt(17.0, 20.0), pt(13.0, 16.0)], st);
    painter.line_segment([pt(3.0, 8.0), pt(7.0, 4.0)], st);
    painter.line_segment([pt(7.0, 4.0), pt(11.0, 8.0)], st);
}

/// Give the click-only widget `id` its own corner shape for the global keyboard focus ring (see
/// [`apply`]): pills and circles would otherwise get the default rounded-rect ring.
pub fn focus_radius(ctx: &egui::Context, id: egui::Id, radius: CornerRadius) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("theme_focus_radius").with(id), radius));
}

/// `ComboBox::icon` painter: a small CHEVRON in TEXT_DIM instead of egui's big solid triangle.
pub fn combo_chevron(ui: &egui::Ui, rect: egui::Rect, _visuals: &egui::style::WidgetVisuals, _open: bool) {
    paint_glyph(ui.painter(), rect.center(), glyph::CHEVRON, 9.5, TEXT_DIM);
}

/// A `side` x `side` (default 22) square secondary-style button with an ink-centred glyph:
/// `BTN` fill, `BTNBD` border, accent border on hover, `SELBD` when pressed (mockup `.btn.sq`).
/// Hand cursor; dims when the `Ui` is disabled. Returns the click response.
pub fn icon_button(ui: &mut egui::Ui, glyph: &str, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let border = if !enabled {
            BORDER
        } else if resp.is_pointer_button_down_on() {
            SELBD
        } else if resp.hovered() {
            ACCENT
        } else {
            BTNBD
        };
        let p = ui.painter();
        p.rect(rect, CornerRadius::same(RADIUS), BTN, Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);
        if enabled && resp.hovered() {
            p.rect_filled(rect.shrink(1.0), CornerRadius::same(RADIUS - 1), HOV);
        }
        if glyph == "\u{2026}" {
            // The `...` menu button: the mockup's plain 13 px "…" in TEXT (`.btn.more`). The font's
            // own ellipsis smears into two blobs at this size, so draw its three dots (light:
            // ~1.7 px, 3.6 px apart; the old "•••" were 2.8 px, 4.5 px apart).
            let c = if enabled { TEXT } else { FAINT };
            for dx in [-3.6_f32, 0.0, 3.6] {
                p.circle_filled(rect.center() + egui::vec2(dx, 1.0), 0.85, c);
            }
        } else {
            paint_glyph(p, rect.center(), glyph, 12.0, if enabled { TEXT_DIM } else { FAINT });
        }
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

// ---- Pills ---------------------------------------------------------------

/// Every status pill and the token it uses (plan D53/D54; the MODIFIED pill is WARN, not DANGER).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillKind {
    /// up to date: GOOD
    Ok,
    /// update available: WARN
    Update,
    /// not installed: TEXT_DIM
    Missing,
    /// `NEEDS <tool>` (session badge, exit 3): DANGER
    Needs,
    /// website: ACCENT
    Web,
    /// auto-update off: TEXT_DIM
    Held,
    /// local changes: WARN
    Modified,
    /// has manual setup steps: ACCENT
    Setup,
    /// not installed and the network is unknown: TEXT_DIM
    Offline,
    /// installed but the fetch failed: DANGER
    FetchFailed,
    /// the installed branch was deleted on the remote: WARN
    BranchGone,
    /// `SETUP NEEDED` (session badge, exit 4): WARN
    SetupNeeded,
}

impl PillKind {
    pub fn color(self) -> Color32 {
        match self {
            PillKind::Ok => GOOD,
            PillKind::Update | PillKind::Modified | PillKind::BranchGone | PillKind::SetupNeeded => WARN,
            PillKind::Missing | PillKind::Held | PillKind::Offline => TEXT_DIM,
            PillKind::Needs | PillKind::FetchFailed => DANGER,
            PillKind::Web | PillKind::Setup => ACCENT,
        }
    }

    /// Default label (drawn uppercase). `Needs` callers usually pass `format!("Needs {tool}")` to [`pill`].
    pub fn label(self) -> &'static str {
        match self {
            PillKind::Ok => "Up to date",
            PillKind::Update => "Update available",
            PillKind::Missing => "Not installed",
            PillKind::Needs => "Needs",
            PillKind::Web => "Website",
            PillKind::Held => "Held",
            PillKind::Modified => "Modified",
            PillKind::Setup => "Setup",
            PillKind::Offline => "Offline",
            PillKind::FetchFailed => "Fetch failed",
            PillKind::BranchGone => "Branch deleted",
            PillKind::SetupNeeded => "Setup needed",
        }
    }
}

/// Height of a pill.
pub const PILL_H: f32 = 20.0;

/// A status pill (mockup `.pill`, enlarged with the cards): 20 pt high, 12 pt Geist Medium, UPPERCASE, +0.03em letter
/// spacing, text `color`, fill `color`@16 %, border `color`@42 %, fully rounded, 7 px side padding.
/// Content-sized; in a `horizontal_wrapped` row pills wrap onto further lines.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    pill_inner(ui, text, color, medium(12.0), 7.0)
}

/// A glyph-only pill (the "held" pause pill: 9 px glyph, 6 px padding).
pub fn pill_glyph(ui: &mut egui::Ui, glyph: &str, color: Color32) -> egui::Response {
    pill_inner(ui, glyph, color, FontId::proportional(10.0), 6.0)
}

fn pill_inner(ui: &mut egui::Ui, text: &str, color: Color32, font: FontId, pad: f32) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        egui::text::TextFormat { font_id: font.clone(), color, extra_letter_spacing: font.size * 0.03, ..Default::default() },
    );
    let galley = ui.painter().layout_job(job);
    let w = galley.size().x + pad * 2.0 + 2.0; // +2: the 1 px border each side
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, PILL_H), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(
            rect,
            CornerRadius::same((PILL_H / 2.0) as u8),
            with_alpha(color, 0.16),
            Stroke::new(1.0_f32, with_alpha(color, 0.42)),
            egui::StrokeKind::Inside,
        );
        p.galley(rect.center() - galley.size() * 0.5, galley, color);
    }
    resp
}

// ---- Button variants -----------------------------------------------------

/// Primary action: solid accent fill, dark bold text.
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(PRIMARY_TEXT).font(bold(13.0)))
        .fill(ACCENT)
        .stroke(Stroke::new(1.0_f32, ACCENT))
}

/// Destructive: transparent fill, red text + red border @ 42 % (mockup `.btn.danger`). The border
/// is a fixed `.stroke()`, which egui keeps in every state: call [`hover_outline`] with `DANGER`
/// on the returned `Response` for the mockup's solid-red hover border.
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(DANGER))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, with_alpha(DANGER, 0.42)))
}

/// Secondary/neutral action: BTN fill. No `.stroke()` on purpose: egui then takes the border from
/// the theme's widget visuals ([`apply`]): BTNBD idle, ACCENT hovered, SELBD pressed, BORDER disabled.
pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(TEXT)).fill(BTN)
}

/// Paint a 1 px `color` border over a hovered, enabled widget's frame (for buttons whose fixed
/// `.stroke()` hides egui's hover border: [`danger_button`] with `DANGER`). Call after `ui.add`.
pub fn hover_outline(ui: &egui::Ui, resp: &egui::Response, color: Color32) {
    if resp.hovered() && ui.is_enabled() && ui.is_rect_visible(resp.rect) {
        ui.painter().rect_stroke(resp.rect, CornerRadius::same(RADIUS), Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
    }
}

/// Mockup `:focus-visible { outline: 1px solid var(--accent); outline-offset: 1px }`, app-wide:
/// at the end of every pass the widget holding keyboard focus gets a 1 px ACCENT ring, 1 px
/// outside its rect, if it is a click-only widget (buttons, tabs, cards; not text edits or
/// sliders, which have their own focus look). egui never gives focus on a mouse click, so this
/// shows for keyboard (Tab) or programmatic (a modal's default button) focus only. Installed
/// once per `Context` by [`apply`].
fn install_focus_ring(ctx: &egui::Context) {
    let flag = egui::Id::new("theme_focus_ring_installed");
    if ctx.data_mut(|d| d.get_temp::<bool>(flag)).is_some() {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(flag, true));
    ctx.on_end_pass(
        "theme_focus_ring",
        std::sync::Arc::new(|ctx: &egui::Context| {
            let Some(id) = ctx.memory(|m| m.focused()) else { return };
            let Some(r) = ctx.read_response(id) else { return };
            if !r.sense.senses_click() || r.sense.senses_drag() {
                return;
            }
            // Skip widgets scrolled out of view (their interaction rect is clipped away).
            if !r.interact_rect.is_positive() || !r.rect.is_positive() {
                return;
            }
            let radius = ctx
                .data(|d| d.get_temp::<CornerRadius>(egui::Id::new("theme_focus_radius").with(id)))
                .map_or(CornerRadius::same(RADIUS + 2), |c| CornerRadius {
                    nw: c.nw.saturating_add(2),
                    ne: c.ne.saturating_add(2),
                    sw: c.sw.saturating_add(2),
                    se: c.se.saturating_add(2),
                });
            ctx.layer_painter(r.layer_id).rect_stroke(
                r.rect.expand(2.0),
                radius,
                Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }),
    );
}

/// Insert `\n` into `text` so it breaks only after a `/` (egui would otherwise prefer breaking at
/// a hyphen, splitting names like `ritz-get`). A single component wider than `max_w` is left to
/// the label's own wrapping.
pub fn wrap_at_slashes(ui: &egui::Ui, text: &str, font: &egui::FontId, max_w: f32) -> String {
    let width = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE).size().x;
    let mut out = String::new();
    let mut line = String::new();
    for part in text.split_inclusive('/') {
        if !line.is_empty() && width(&format!("{line}{part}")) > max_w {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        line.push_str(part);
    }
    out.push_str(&line);
    out
}

/// How long [`flashing`] stays true.
pub const FLASH_SECS: f64 = 1.5;

/// Mark `key` as "just happened" (e.g. a Copy button was pressed). Pair with [`flashing`].
pub fn flash(ctx: &egui::Context, key: impl std::hash::Hash) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("theme_flash", key)), now));
}

/// True for [`FLASH_SECS`] after [`flash`]`(ctx, key)` (and schedules the repaint that ends it),
/// e.g. to show "Copied" on a button.
pub fn flashing(ctx: &egui::Context, key: impl std::hash::Hash) -> bool {
    let id = egui::Id::new(("theme_flash", key));
    let Some(t) = ctx.data_mut(|d| d.get_temp::<f64>(id)) else { return false };
    let left = FLASH_SECS - (ctx.input(|i| i.time) - t);
    if left > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left + 0.02));
        true
    } else {
        ctx.data_mut(|d| d.remove_temp::<f64>(id));
        false
    }
}

/// An UPPERCASE card / group title: ACCENT, 12 px Geist Bold, +0.04em letter spacing.
pub fn section_label(text: &str) -> RichText {
    RichText::new(text.to_uppercase()).color(ACCENT).font(bold(12.0)).extra_letter_spacing(0.5)
}

/// Gray placeholder text for a text box's `hint_text`. Needed because the theme's
/// `override_text_color` otherwise paints the hint the same near-white as real text —
/// an explicit colour wins over the override.
pub fn placeholder(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(DIM)
}

// ---- Panes: columns & cards ---------------------------------------------
//
// The containment rule ("everything stays in its
// container"): every page column and every card is a *pane* with a fixed width taken from
// its parent, and its contents are clipped to it. Why: egui grows a `Ui` (and every
// enclosing `Ui`) to fit a widget that is wider than the space it was given, and plain
// `ui.columns` doesn't clip — so one too-wide row (the Overlay tab's Drift Counter radios)
// used to widen its card past the column and paint over the neighbouring column. Inside a
// pane only the pane's own width counts: rows shrink, wrap or stack to it, and whatever
// still doesn't fit is cut at the pane edge instead of spilling into another pane.

/// A child `Ui` exactly `rect.width()` wide (top at `rect.top()`, growing down), painted
/// only inside `clip_x` horizontally. Its overflow never reaches the parent: the caller
/// allocates the fixed width itself.
pub fn pane_ui(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    clip_x: egui::Rangef,
    layout: egui::Layout,
    salt: impl std::hash::Hash,
) -> egui::Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(salt).max_rect(rect).layout(layout));
    child.set_width(rect.width()); // min == max: the pane neither shrinks nor grows
    let clip = child.clip_rect();
    child.shrink_clip_rect(egui::Rect::from_x_y_ranges(clip_x, clip.y_range()));
    child
}

/// Like `ui.columns`, but each column is a contained pane: exactly `1/n` of the width (minus
/// the `item_spacing.x` gaps), clipped to its own slot, and the row allocates exactly the
/// available width — a too-wide widget in one column can neither widen it nor paint into
/// the next. Use it for page columns *and* for the label | control halves of a row.
pub fn columns<R>(ui: &mut egui::Ui, n: usize, add: impl FnOnce(&mut [egui::Ui]) -> R) -> R {
    let n = n.max(1);
    let gap = ui.spacing().item_spacing.x;
    let total = ui.available_width().max(0.0);
    let col_w = ((total - gap * (n as f32 - 1.0)) / n as f32).max(0.0);
    columns_sized(ui, &vec![col_w; n], gap, add)
}

/// A `ScrollArea` for the card grid: a solid (non-floating) 6 px bar that is visible whenever the
/// content overflows and whose gutter egui takes out of the content width, so the bar never
/// draws over the right-hand cards. Call on the `Ui` the area is shown in.
///
/// Also recolours the bar (mockup `::-webkit-scrollbar-thumb`): egui paints it from the shared
/// widget visuals, so thumb = `BTNBD` (hover/drag a step lighter) on a transparent track
/// (`extreme_bg_color`). Returns the original visuals; the caller puts them back inside the
/// area's content (`*ui.visuals_mut() = orig`) so no other widget changes look.
#[must_use]
pub fn solid_scroll_style(ui: &mut egui::Ui) -> egui::Visuals {
    let orig = ui.visuals().clone();
    let s = &mut ui.style_mut().spacing.scroll;
    *s = egui::style::ScrollStyle::solid();
    s.bar_inner_margin = 3.0;
    let v = ui.visuals_mut();
    v.extreme_bg_color = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = BTNBD;
    v.widgets.hovered.bg_fill = SCROLL_THUMB_HOVER;
    v.widgets.active.bg_fill = SCROLL_THUMB_HOVER;
    orig
}

/// Scrollbar thumb while hovered / dragged: `BTNBD` stepped up a little.
pub const SCROLL_THUMB_HOVER: Color32 = Color32::from_rgb(0x4A, 0x51, 0x5C);

/// [`columns`] with explicit widths (e.g. `[groups_w, 300.0]`) and gap. The row allocates
/// `sum(widths) + gaps` wide; each pane is clipped to its slot, extends down to the parent's
/// bottom (so a pane can paint a full-height background) and uses a top-down justified layout.
pub fn columns_sized<R>(
    ui: &mut egui::Ui,
    widths: &[f32],
    gap: f32,
    add: impl FnOnce(&mut [egui::Ui]) -> R,
) -> R {
    let top_left = ui.cursor().min;
    let bottom = ui.max_rect().bottom().max(top_left.y);
    let salt = ui.next_auto_id();
    let mut x = top_left.x;
    let mut cols: Vec<egui::Ui> = widths
        .iter()
        .enumerate()
        .map(|(i, &w)| {
            let w = w.max(0.0);
            let rect = egui::Rect::from_min_max(egui::pos2(x, top_left.y), egui::pos2(x + w, bottom));
            // Half the gap of slack each side: focus rings / hover washes stay visible, but
            // two neighbouring columns' clip rects never overlap.
            let clip_x = egui::Rangef::new(x - gap * 0.5, x + w + gap * 0.5);
            x += w + gap;
            let layout = egui::Layout::top_down_justified(egui::Align::LEFT);
            pane_ui(ui, rect, clip_x, layout, (salt, "theme_col", i))
        })
        .collect();
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let r = add(&mut cols);
    let h = cols.iter().map(|c| c.min_rect().bottom() - top_left.y).fold(0.0_f32, f32::max);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(top_left, egui::vec2(total, h)));
    r
}

/// A framed pane: `frame` (fill / stroke / radius / margin) as wide as the parent's available
/// width, its body laid out in, and clipped to, the *inner* width. This is the core of [`card`];
/// use it directly for program tiles and the side panel. A row that is too wide is cut at the
/// frame edge, it never widens the frame. Size rows from `ui.available_width()`.
pub fn framed<R>(ui: &mut egui::Ui, frame: egui::Frame, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let margin = frame.total_margin();
    let inner_w = (ui.available_width() - margin.sum().x).max(0.0);
    frame
        .show(ui, |ui| {
            let top_left = ui.cursor().min;
            let rect = egui::Rect::from_min_max(
                top_left,
                egui::pos2(top_left.x + inner_w, ui.max_rect().bottom().max(top_left.y)),
            );
            // Clip just inside the frame's stroke, so hover washes in the inner margin show.
            let slack = (margin.left as f32 - frame.stroke.width - 1.0).max(0.0);
            let clip_x = egui::Rangef::new(rect.left() - slack, rect.right() + slack);
            let salt = ui.auto_id_with("theme_framed");
            let mut body_ui = pane_ui(ui, rect, clip_x, *ui.layout(), salt);
            let r = body(&mut body_ui);
            let h = (body_ui.min_rect().bottom() - top_left.y).max(0.0);
            ui.advance_cursor_after_rect(egui::Rect::from_min_size(top_left, egui::vec2(inner_w, h)));
            r
        })
        .inner
}

/// A bordered card with a blue [`section_label`] title, followed by a uniform
/// 8px gap — the Co-Op tab's category styling, reused across tabs.
///
/// The card is a pane: exactly as wide as its parent's available width (its column), and
/// its body is laid out in, and clipped to, the card's inner width — a row that is too
/// wide is cut at the card edge, it never widens the card. Size the body's rows from
/// `ui.available_width()` (the row helpers below all do).
///
/// The 8px trailing space is the *only* inter-card gap, so callers must zero
/// the container's vertical item spacing (`ui.spacing_mut().item_spacing.y = 0.0`)
/// before stacking cards; otherwise egui adds its own spacing on top. The card
/// sets its own inner spacing, independent of that outer zero.
pub fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    framed(ui, egui::Frame::group(ui.style()), |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP; // comfortable spacing inside the card
        ui.label(section_label(title));
        ui.add_space(ROW_GAP);
        body(ui);
    });
    ui.add_space(GAP);
}

// ---- Settings rows -------------------------------------------------------

/// A left-column row label, laid out to line up with the control in the right
/// column of a two-`columns` settings row. Returns the label response (for
/// hover tooltips).
///
/// Why not a plain `ui.label`: `ui.columns` top-aligns each column, so a bare
/// label sits at the row's top edge while the control beside it (a slider /
/// combobox / spinner, ~`interact_size.y` tall) is vertically centered — the
/// label then reads as floating above the control. This allocates the label a
/// row of the standard control height and vertically centers it, so the two
/// line up. `left_to_right` (not the columns' justified layout) also stops a
/// wrapping label from spreading its letters across the line.
///
/// ponytail: fixed height = one control row; a label long enough to wrap to two
/// lines overflows downward. Fine for the short settings labels in use; give it
/// its own min-height growth if multi-line labels ever appear.
pub fn row_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.add(egui::Label::new(label).wrap()),
    )
    .inner
}

// ---- Apply ---------------------------------------------------------------

/// Install the Graphite visuals + type scale. Call once at startup (after [`install_fonts`]).
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();

    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = HOV;
    v.hyperlink_color = ACCENT;

    v.selection.bg_fill = SEL;
    v.selection.stroke = Stroke::new(1.0_f32, SELBD);

    let round = CornerRadius::same(7);

    // Non-interactive surfaces (labels, separators, group frames).
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, DIM);
    v.widgets.noninteractive.corner_radius = round;

    // Resting interactive widgets.
    v.widgets.inactive.bg_fill = BTN;
    v.widgets.inactive.weak_bg_fill = BTN;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BTNBD);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.corner_radius = round;

    // Hovered.
    v.widgets.hovered.bg_fill = BTN;
    v.widgets.hovered.weak_bg_fill = HOV;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.corner_radius = round;

    // Active / pressed.
    v.widgets.active.bg_fill = BTN;
    v.widgets.active.weak_bg_fill = HOV;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, SELBD);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.active.corner_radius = round;

    // Open (combo boxes, menus).
    v.widgets.open.bg_fill = FIELD;
    v.widgets.open.weak_bg_fill = FIELD;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.open.corner_radius = round;

    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        use egui::FontFamily::{Monospace, Proportional};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(18.0, Proportional)),
            (TextStyle::Body, FontId::new(13.0, Proportional)),
            (TextStyle::Button, FontId::new(13.0, Proportional)),
            (TextStyle::Small, FontId::new(11.0, Proportional)),
            (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        ]
        .into();
        // Mockup `.btn`: 22 px high, 9 px side padding. 13 px Geist lays out 17 px tall; with the old
        // vertical padding of 5 every themed button grew to 27 px. Small on purpose: egui rounds the
        // frame margin (padding - 1 px stroke) to whole px, 2.5 would give 23; `interact_size.y` pads it to 22.
        s.spacing.button_padding = egui::vec2(9.0, 2.0);
        s.spacing.interact_size.y = 22.0;
    });
    install_focus_ring(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Context;

    fn ctx() -> Context {
        let ctx = Context::default();
        install_fonts(&ctx);
        apply(&ctx);
        ctx
    }

    /// Run one pass of `ui` in a CentralPanel and return the shapes + whatever `f` returns.
    fn frame<R>(ctx: &Context, mut f: impl FnMut(&mut egui::Ui) -> R) -> (egui::FullOutput, R) {
        let mut r = None;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| r = Some(f(ui)));
        });
        (out, r.unwrap())
    }

    /// `centered_pos` puts the centre of what the glyph draws on the cell centre, even for glyphs
    /// whose layout-box centre is elsewhere.
    #[test]
    fn centered_pos_puts_the_ink_centre_on_the_cell_centre() {
        let ctx = ctx();
        let cell = egui::pos2(50.0, 40.0);
        let mut off_centre = 0;
        let (_, checked) = frame(&ctx, |ui| {
            let p = ui.painter();
            let mut checked = 0;
            for g in [glyph::GEAR, glyph::GITHUB, glyph::EXT, glyph::TRASH, glyph::FOLDER, glyph::BUG, glyph::CHEVRON, glyph::PAUSE, "\u{2197}"] {
                let font = FontId::proportional(13.0);
                let pos = centered_pos(p, cell, g, &font);
                let galley = p.layout_no_wrap(g.to_owned(), font, Color32::WHITE);
                assert!(galley.mesh_bounds.is_positive(), "{g:?} draws nothing");
                let ink = pos + galley.mesh_bounds.center().to_vec2();
                assert!((ink - cell).length() < 0.5, "{g:?}: ink centre {ink:?} vs cell {cell:?}");
                if (galley.size() * 0.5 - galley.mesh_bounds.center().to_vec2()).length() > 0.5 {
                    off_centre += 1;
                }
                checked += 1;
            }
            checked
        });
        assert_eq!(checked, 9);
        assert!(off_centre > 0, "no tested glyph has a layout-box centre different from its ink centre");
    }

    /// Root cause of the "tall Change... button": not the U+2026 glyph (Geist has it; the line is
    /// 17 px tall with or without it) but `button_padding.y`. Every themed button is CTRL_H high.
    #[test]
    fn themed_buttons_are_ctrl_h_high_with_or_without_an_ellipsis() {
        let ctx = ctx();
        let mut rects = Vec::new();
        for _ in 0..2 {
            let (_, r) = frame(&ctx, |ui| {
                ["Change", "Change\u{2026}", "Checking\u{2026}", "Open folder"]
                    .iter()
                    .flat_map(|t| [ui.add(secondary_button(*t)), ui.add(primary_button(*t)), ui.add(danger_button(*t))])
                    .map(|r| r.rect.height())
                    .collect::<Vec<_>>()
            });
            rects = r;
        }
        assert!(rects.iter().all(|h| (*h - CTRL_H).abs() < 0.01), "button heights: {rects:?}");
    }

    /// The focused click-only widget gets the ACCENT outline (1 px outside its rect); an
    /// unfocused one does not.
    #[test]
    fn focused_button_gets_the_accent_ring() {
        let has_ring = |out: &egui::FullOutput| {
            out.shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.stroke.color == ACCENT && r.stroke.width == 1.0 && r.fill == Color32::TRANSPARENT))
        };
        let ctx = ctx();
        let mut id = None;
        let mut last = None;
        for _ in 0..3 {
            let (out, r) = frame(&ctx, |ui| ui.add(secondary_button("Cancel")));
            id = Some(r.id);
            last = Some(out);
        }
        assert!(!has_ring(&last.unwrap()), "no ring without focus");
        ctx.memory_mut(|m| m.request_focus(id.unwrap()));
        let mut ring = false;
        for _ in 0..3 {
            let (out, _) = frame(&ctx, |ui| ui.add(secondary_button("Cancel")));
            ring = has_ring(&out);
        }
        assert!(ring, "focused button has the outline");
    }

    #[test]
    fn flash_lasts_flash_secs() {
        let ctx = ctx();
        assert!(!flashing(&ctx, "k"));
        flash(&ctx, "k");
        assert!(flashing(&ctx, "k"));
        assert!(!flashing(&ctx, "other"));
    }
}
