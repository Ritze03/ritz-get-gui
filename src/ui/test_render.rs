//! Test-only harness: run the whole app
//! frame headlessly in a real `egui::Context` (the app's fonts and theme), check that nothing
//! paints outside its pane, and optionally rasterise the frame to a PNG (set
//! `RITZ_GET_GUI_SNAPSHOT_DIR`) so a layout can be looked at without a window.
//! Rule: every region is its own clipped pane (nested panes).
//!
//! Widths (D55): 1280x800 (side beside, 3 card columns), 900x700 (side beside, 2 columns),
//! 600x800 (below both mockup breakpoints: side stacked, top bar in its narrow form).
//!
//! The whole-app test also runs at `pixels_per_point` 1.5 and 2.0 ([`SCALES`]): the logical size
//! stays W x H, the layout is in points, so the pane rule holds at every display scale.
//!
//! `cargo test pane` selects the tests here (and the helper-level one) by name.

use std::collections::HashMap;

use egui::{Color32, ColorImage, Context, FullOutput, Pos2, Rect, TextureId};

use crate::app::AppState;

/// The three D55 test sizes.
pub const SIZES: [(f32, f32); 3] = [(1280.0, 800.0), (900.0, 700.0), (600.0, 800.0)];

/// The display scales the whole-app test runs at (`pixels_per_point`).
pub const SCALES: [f32; 3] = [1.0, 1.5, 2.0];

/// A context with the app's fonts and Graphite theme (as `App::new` sets them up).
/// Use a fresh one per [`run`]: the font atlas is only sent whole on a context's first frame, so
/// a reused context's snapshot would have no glyphs.
pub fn context() -> Context {
    context_at(1.0)
}

/// [`context`] at `pixels_per_point` = `ppp` (takes effect from the first pass [`run_ctx`] runs).
pub fn context_at(ppp: f32) -> Context {
    let ctx = Context::default();
    ctx.set_pixels_per_point(ppp);
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx);
    egui_extras::install_image_loaders(&ctx);
    ctx
}

/// Run `frame` for a few passes of a `w`x`h` window (so fonts and remembered sizes settle) and
/// return the last frame plus every texture it references.
pub fn run_ctx(
    ctx: &Context,
    w: f32,
    h: f32,
    mut frame: impl FnMut(&Context),
) -> (FullOutput, HashMap<TextureId, ColorImage>) {
    crate::icons::letters_only_for_this_thread(); // deterministic: no async icon textures
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    let mut last = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| frame(ctx));
        for (id, delta) in &out.textures_delta.set {
            let egui::ImageData::Color(img) = &delta.image;
            match delta.pos {
                None => {
                    textures.insert(*id, (**img).clone());
                }
                Some([x0, y0]) => {
                    let t = textures.get_mut(id).expect("patch for an unknown texture");
                    for y in 0..img.size[1] {
                        for x in 0..img.size[0] {
                            t.pixels[(y0 + y) * t.size[0] + x0 + x] = img.pixels[y * img.size[0] + x];
                        }
                    }
                }
            }
        }
        last = Some(out);
    }
    (last.unwrap(), textures)
}

/// [`run_ctx`] for a page drawn inside a `CentralPanel` (helper-level tests).
pub fn run(
    ctx: &Context,
    w: f32,
    h: f32,
    mut page: impl FnMut(&mut egui::Ui),
) -> (FullOutput, HashMap<TextureId, ColorImage>) {
    run_ctx(ctx, w, h, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| page(ui));
    })
}

/// Each shape's visible part (bounding box ∩ clip rect), skipping empty ones and the
/// `CentralPanel`'s own background (the only shape that legitimately spans every column).
pub fn visible_rects(out: &FullOutput) -> Vec<Rect> {
    out.shapes
        .iter()
        .filter(|c| !matches!(&c.shape, egui::Shape::Rect(r) if r.rect.min == Pos2::ZERO && r.rect.contains_rect(c.clip_rect)))
        .map(|c| c.shape.visual_bounding_rect().intersect(c.clip_rect))
        .filter(|r| r.is_positive())
        .collect()
}

/// Rasterise the frame (no anti-aliasing beyond egui's feathering) and save it as PNG to
/// `$RITZ_GET_GUI_SNAPSHOT_DIR/<name>.png`. A no-op when the variable isn't set.
pub fn snapshot(ctx: &Context, out: &FullOutput, textures: &HashMap<TextureId, ColorImage>, w: u32, h: u32, name: &str) {
    let Ok(dir) = std::env::var("RITZ_GET_GUI_SNAPSHOT_DIR") else { return };
    std::fs::create_dir_all(&dir).expect("create snapshot dir");
    // `w`/`h` are points: rasterise at the context's scale (physical pixels).
    let ppp = ctx.pixels_per_point();
    let (w, h) = ((w as f32 * ppp).round() as u32, (h as f32 * ppp).round() as u32);
    let prims = ctx.tessellate(out.shapes.clone(), ppp);
    let mut px = vec![[0.0_f32; 4]; (w * h) as usize];
    for p in &prims {
        let egui::epaint::Primitive::Mesh(mesh) = &p.primitive else { continue };
        let Some(tex) = textures.get(&mesh.texture_id) else { continue };
        let clip = p.clip_rect.intersect(Rect::from_min_size(Pos2::ZERO, egui::vec2(w as f32, h as f32)));
        for tri in mesh.indices.chunks_exact(3) {
            let v = [&mesh.vertices[tri[0] as usize], &mesh.vertices[tri[1] as usize], &mesh.vertices[tri[2] as usize]];
            let (a, b, c) = (v[0].pos, v[1].pos, v[2].pos);
            let area = (b - a).x * (c - a).y - (b - a).y * (c - a).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let bb = Rect::from_points(&[a, b, c]).intersect(clip);
            if !bb.is_positive() {
                continue;
            }
            for y in bb.top().floor() as u32..bb.bottom().ceil().min(h as f32) as u32 {
                for x in bb.left().floor() as u32..bb.right().ceil().min(w as f32) as u32 {
                    let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    if !clip.contains(p) {
                        continue;
                    }
                    let w0 = ((b - p).x * (c - p).y - (b - p).y * (c - p).x) / area;
                    let w1 = ((c - p).x * (a - p).y - (c - p).y * (a - p).x) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let lerp = |f: &dyn Fn(usize) -> f32| w0 * f(0) + w1 * f(1) + w2 * f(2);
                    let u = lerp(&|i| v[i].uv.x);
                    let vv = lerp(&|i| v[i].uv.y);
                    let tx = ((u * tex.size[0] as f32) as usize).min(tex.size[0] - 1);
                    let ty = ((vv * tex.size[1] as f32) as usize).min(tex.size[1] - 1);
                    let t: Color32 = tex.pixels[ty * tex.size[0] + tx];
                    let mut src = [0.0_f32; 4];
                    for (k, s) in src.iter_mut().enumerate() {
                        let vc = lerp(&|i| v[i].color.to_array()[k] as f32 / 255.0);
                        *s = vc * t.to_array()[k] as f32 / 255.0;
                    }
                    let d = &mut px[(y * w + x) as usize];
                    for k in 0..4 {
                        d[k] = src[k] + d[k] * (1.0 - src[3]); // premultiplied "over"
                    }
                }
            }
        }
    }
    let mut img = image::RgbaImage::new(w, h);
    for (i, p) in px.iter().enumerate() {
        let c = |f: f32| (f.clamp(0.0, 1.0) * 255.0) as u8;
        img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba([c(p[0]), c(p[1]), c(p[2]), 255]));
    }
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    img.save(&path).expect("write snapshot");
}

/// Render the whole app for `state` at `w`x`h` on a fresh context. Returns everything a test needs.
pub fn render_app(state: &AppState, w: f32, h: f32) -> (Context, FullOutput, HashMap<TextureId, ColorImage>, super::PaneRects) {
    render_app_at(state, w, h, 1.0)
}

/// [`render_app`] at `pixels_per_point` = `ppp` (the window stays `w` x `h` points).
pub fn render_app_at(state: &AppState, w: f32, h: f32, ppp: f32) -> (Context, FullOutput, HashMap<TextureId, ColorImage>, super::PaneRects) {
    let ctx = context_at(ppp);
    let mut rects = None;
    let (out, tex) = run_ctx(&ctx, w, h, |ctx| {
        let mut actions = Vec::new();
        rects = Some(super::draw(ctx, state, &mut actions));
    });
    (ctx, out, tex, rects.expect("ui::draw ran"))
}

/// Every shape's visible part must lie inside one of the frame's panes (±`TOL` px for strokes and
/// feathering), and inside the window. A shape that straddles two panes (e.g. the groups card
/// grid painting into the side panel, or a row poking out of the log drawer) fails.
/// The CentralPanel's own background (exactly `central`) is the one legitimate straddler.
pub fn assert_inside_panes(out: &FullOutput, rects: &super::PaneRects, w: f32, h: f32, what: &str) {
    const TOL: f32 = 4.5;
    let mut panes: Vec<(&str, Rect)> = vec![
        ("top", rects.top),
        ("status", rects.status),
        ("groups", rects.groups),
    ];
    if let Some(r) = rects.restart {
        panes.push(("restart", r));
    }
    if let Some(r) = rects.banner {
        panes.push(("banner", r));
    }
    if let Some(r) = rects.side {
        panes.push(("side", r));
    }
    if rects.stacked {
        // The stacked body scrolls as a whole: its solid scroll bar sits in the gutter right of both panes.
        panes.push(("scroll bar", Rect::from_min_max(egui::pos2(rects.groups.right(), rects.central.top()), rects.central.max)));
    }
    let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h)).expand(TOL);
    let close = |a: Rect, b: Rect| (a.min - b.min).abs().max_elem() < 0.6 && (a.max - b.max).abs().max_elem() < 0.6;
    for c in &out.shapes {
        if matches!(&c.shape, egui::Shape::Rect(r) if close(r.rect, rects.central)) {
            continue; // the CentralPanel background
        }
        let r = c.shape.visual_bounding_rect().intersect(c.clip_rect);
        if !r.is_positive() {
            continue;
        }
        assert!(window.contains_rect(r), "{what}: a shape paints outside the window: {r:?} (window {w}x{h})");
        assert!(
            panes.iter().any(|(_, p)| r.left() >= p.left() - TOL
                && r.right() <= p.right() + TOL
                && r.top() >= p.top() - TOL
                && r.bottom() <= p.bottom() + TOL),
            "{what}: a shape paints across panes: {r:?}\npanes: {panes:#?}"
        );
    }
}

/// Like [`render_app`] but only the panes (no modal overlays): the strict pane-rule baseline.
pub fn render_panes(state: &AppState, w: f32, h: f32) -> (Context, FullOutput, HashMap<TextureId, ColorImage>, super::PaneRects) {
    render_panes_at(state, w, h, 1.0)
}

/// [`render_panes`] at `pixels_per_point` = `ppp`.
pub fn render_panes_at(state: &AppState, w: f32, h: f32, ppp: f32) -> (Context, FullOutput, HashMap<TextureId, ColorImage>, super::PaneRects) {
    let ctx = context_at(ppp);
    let mut rects = None;
    let (out, tex) = run_ctx(&ctx, w, h, |ctx| {
        let mut actions = Vec::new();
        rects = Some(super::draw_panes(ctx, state, &mut actions));
    });
    (ctx, out, tex, rects.expect("ui::draw_panes ran"))
}

/// The shapes of `full` that `base` does not have: the overlay (modal / popover) layers.
/// Shapes are matched by clip rect + bounding box, which is exact for a deterministic re-render.
pub fn overlay_shapes<'a>(full: &'a FullOutput, base: &FullOutput) -> Vec<&'a egui::epaint::ClippedShape> {
    let key = |c: &egui::epaint::ClippedShape| -> [u32; 8] {
        let (cl, r) = (c.clip_rect, c.shape.visual_bounding_rect());
        [cl.min.x, cl.min.y, cl.max.x, cl.max.y, r.min.x, r.min.y, r.max.x, r.max.y].map(f32::to_bits)
    };
    let known: std::collections::HashSet<[u32; 8]> = base.shapes.iter().map(key).collect();
    full.shapes.iter().filter(|c| !known.contains(&key(c))).collect()
}

/// A finished Remove / DeleteData run of `id` (section/name) behind the progress dialog, in `phase`
/// (`Done` shows `ok_line` in its box). The listing is left as it is: pick a program that is not
/// installed and has data for "removed, data left". A finished DeleteData also drops the entry's
/// data (the refreshed listing after the delete). Like the real app, neither command puts a bar on
/// the program's card (`begin_progress` ignores them), so no per-card progress is left over from the
/// install hook this starts from, and the run's log is that command's alone.
pub fn finished_run(s: &mut AppState, id: &str, verb: crate::app::Verb, phase: crate::app::RunPhase, ok_line: &str) {
    use crate::app::{RunPhase, Verb};
    let name = id.rsplit('/').next().unwrap_or(id).to_owned();
    s.apply_shot_hook(&format!("install-failed={id}"));
    let cmd = match verb {
        Verb::DeleteData => crate::model::Command::DeleteData { name: name.clone() },
        _ => crate::model::Command::Remove { name: name.clone() },
    };
    if let Some(run) = &mut s.run {
        run.verb = verb;
        run.cmd = cmd.clone();
    }
    // the hook's card bar ("FAILED") and its install log lines are not this run's
    s.progress.clear();
    s.progress_order.clear();
    if let Some(at) = s.log.iter().rposition(|l| l.kind == crate::model::LogKind::Cmd) {
        s.log.truncate(at + 1);
        s.log[at].text = format!("$ {cmd}");
    }
    if verb == Verb::DeleteData && matches!(phase, RunPhase::Done { .. }) {
        if let Some(e) = s.listing.sections.iter_mut().flat_map(|x| x.entries.iter_mut()).find(|e| e.name == name) {
            e.data.clear();
        }
    }
    s.push_log(crate::model::LogKind::Ok, ok_line.to_owned());
    s.set_run_phase(phase);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{RunPhase, Verb};
    use crate::model::{Badge, LastRun, Modal, Status, View};

    fn fixture(name: &str) -> crate::model::Listing {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        crate::porcelain::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// The real full fixture (17 entries + an unlisted clone), online, a selection.
    fn full() -> AppState {
        AppState {
            listing: fixture("full.porcelain"),
            selected: Some("Helpers/ts6tray".into()),
            checked_at: Some(std::time::Instant::now()),
            ..AppState::default()
        }
    }

    fn entry_mut<'a>(s: &'a mut AppState, name: &str) -> &'a mut crate::model::Entry {
        s.listing.sections.iter_mut().flat_map(|x| x.entries.iter_mut()).find(|e| e.name == name).unwrap()
    }

    /// One whole-app scenario. `overlay`: it draws something on top of the panes (a modal or an
    /// open popover) that the pane rule does not cover.
    struct Scenario {
        name: &'static str,
        build: fn(&mut AppState),
        overlay: bool,
    }

    const SCENARIOS: &[Scenario] = &[
        Scenario { name: "main", build: |_| {}, overlay: false },
        Scenario { name: "settings", build: |s| s.view = View::Settings, overlay: false },
        // Restart bar AND the not-found banner at once (the original combined state).
        Scenario { name: "restart_and_banner", build: |s| { s.restart = true; s.ritz_get_missing = true; }, overlay: false },
        Scenario { name: "restart", build: |s| s.restart = true, overlay: false },
        // The real flow of a bogus RITZ_GET: nothing listed yet, the banner, actions off.
        Scenario { name: "notfound", build: |s| { s.listing = Default::default(); s.selected = None; s.ritz_get_missing = true; s.checked_at = None; }, overlay: false },
        Scenario { name: "settings_notfound", build: |s| { s.view = View::Settings; s.ritz_get_missing = true; s.restart = true; }, overlay: false },
        Scenario { name: "modal_reset", build: |s| { s.dir = Some("/home/u/.local/share/ritz-get".into()); s.modal = Some(Modal::Reset { id: "Software/geofinder".into() }); s.selected = Some("Software/geofinder".into()); }, overlay: true },
        // Delete program data: the confirmation (ritz: two folders; SteamAlt: installed, so Delete is off), and the
        // progress dialog after a Remove that left data (Done + "Delete data...") and of the delete itself.
        Scenario { name: "modal_delete_data", build: |s| s.apply_shot_hook("deletedata=Software/ritz"), overlay: true },
        Scenario { name: "modal_delete_data_installed", build: |s| s.apply_shot_hook("deletedata=Software/SteamAlt"), overlay: true },
        Scenario {
            name: "modal_delete_data_many",
            build: |s| {
                s.apply_shot_hook("deletedata=Software/ritz");
                let e = entry_mut(s, "ritz");
                for i in 0..14 {
                    e.data.push(crate::model::DataPath { path: format!("~/.local/share/ritz/some/deeply/nested/profile-folder-number-{i}"), kb: (i % 3 != 0).then_some(1000 * i) });
                }
            },
            overlay: true,
        },
        Scenario {
            name: "modal_remove_done_with_data",
            build: |s| finished_run(s, "Software/ritz", Verb::Remove, RunPhase::Done { menu: false }, "ritz: removed"),
            overlay: true,
        },
        Scenario {
            name: "modal_delete_data_running",
            build: |s| { finished_run(s, "Software/ritz", Verb::DeleteData, RunPhase::Running, "ritz: deleting"); s.running = None; },
            overlay: true,
        },
        Scenario {
            name: "modal_delete_data_done",
            build: |s| finished_run(s, "Software/ritz", Verb::DeleteData, RunPhase::Done { menu: false }, "ritz: data deleted"),
            overlay: true,
        },
        Scenario {
            name: "modal_delete_data_failed",
            build: |s| finished_run(s, "Software/SteamAlt", Verb::DeleteData, RunPhase::Failed { reason: "uninstall first: ritz-get remove SteamAlt".into() }, "x"),
            overlay: true,
        },
        // The progress dialog in every state (install: running / done = the 2a question / failed / missing
        // tools with and without a `deps:` command; Update all: running / finished), and the same running
        // dialogs with their log open (long lines, 60 of them).
        Scenario { name: "modal_deps", build: |s| s.apply_shot_hook("deps=Software/ForzaTelemetryV3Launcher"), overlay: true },
        Scenario { name: "modal_deps_none", build: |s| s.apply_shot_hook("deps-none=Software/ForzaTelemetryV3Launcher"), overlay: true },
        Scenario { name: "modal_install_running", build: |s| s.apply_shot_hook("install-running=Software/ritz"), overlay: true },
        Scenario { name: "modal_install_done", build: |s| s.apply_shot_hook("install-done=Software/ritz"), overlay: true },
        Scenario { name: "modal_install_failed", build: |s| s.apply_shot_hook("install-failed=Software/ritz"), overlay: true },
        Scenario { name: "modal_updateall_running", build: |s| s.apply_shot_hook("updateall-running"), overlay: true },
        Scenario { name: "modal_updateall_done", build: |s| s.apply_shot_hook("updateall-done"), overlay: true },
        Scenario {
            name: "modal_install_log",
            build: |s| {
                s.apply_shot_hook("install-running=Software/ritz");
                s.run_log_open = true;
                for i in 0..60 {
                    s.push_log(crate::model::LogKind::Err, format!("line {i}: FAILED (needs: a-really-long-tool-name, another-really-long-tool-name, and-more-tools-and-more-tools-and-more)"));
                }
            },
            overlay: true,
        },
        Scenario {
            name: "modal_updateall_log",
            build: |s| {
                s.apply_shot_hook("updateall-running");
                s.run_log_open = true;
                for i in 0..60 {
                    s.push_log(crate::model::LogKind::Info, format!("line {i}: ritz-get-gui: building {}", "x".repeat(i * 5)));
                }
            },
            overlay: true,
        },
        Scenario { name: "modal_log", build: |s| s.apply_shot_hook("log"), overlay: true },
        // A run sent to the background: no dialog; the status bar (and, for Update all, the top bar) offer the way back.
        Scenario { name: "hidden_run", build: |s| { s.apply_shot_hook("install-running=Software/ritz"); s.modal = None; }, overlay: false },
        Scenario { name: "hidden_updateall", build: |s| { s.apply_shot_hook("updateall-running"); s.modal = None; }, overlay: false },
        Scenario { name: "refused", build: |s| { s.listing = Default::default(); s.selected = None; s.checked_at = None; s.refused = Some(2); }, overlay: true },
        // The `...` popover of a held card with an update (Branch + Auto-update + the action list).
        Scenario { name: "popover", build: |s| s.apply_shot_hook("menu=Software/yt-dlp-gui"), overlay: true },
        Scenario { name: "popover_website", build: |s| s.apply_shot_hook("menu=Websites/cs2-multimap"), overlay: true },
        Scenario { name: "popover_missing", build: |s| s.apply_shot_hook("menu=Software/ritz"), overlay: true },
        // A command is running: log header shows it, every control disabled.
        Scenario { name: "busy", build: |s| s.apply_shot_hook("busy"), overlay: false },
        // Per-card progress bars: a frozen Update all (every bar kind) / a single running update with
        // the side panel's Actions row showing it.
        Scenario { name: "progress", build: |s| s.apply_shot_hook("progress"), overlay: false },
        Scenario {
            name: "progress_single",
            build: |s| {
                let cmd = crate::model::Command::Update { name: Some("ts6tray".into()) };
                s.begin_progress(&cmd);
                s.busy = true;
                s.running = Some(cmd);
                s.selected = Some("Helpers/ts6tray".into());
            },
            overlay: false,
        },
        Scenario {
            name: "offline",
            build: |s| {
                s.listing = fixture("offline.porcelain");
                s.checked_at = None;
            },
            overlay: false,
        },
        Scenario {
            name: "fetch_failed",
            build: |s| {
                entry_mut(s, "gamescope-ritz").status = Status::FetchFailed;
                entry_mut(s, "SteamAlt").status = Status::FetchFailed;
                s.selected = Some("Software/SteamAlt".into());
            },
            overlay: false,
        },
        Scenario {
            name: "last_failed",
            build: |s| {
                s.last_run = Some(LastRun {
                    time: "2026-10-24T03:00:01".into(),
                    updated: 1,
                    skipped: 1,
                    failed: 2,
                    failed_names: vec!["geofinder".into(), "ts6tray".into()],
                });
            },
            overlay: false,
        },
        Scenario { name: "empty", build: |s| s.apply_shot_hook("empty"), overlay: false },
        // Several `Needs` pills on one card (they wrap), more on others, and SETUP NEEDED.
        Scenario { name: "needs", build: |s| s.apply_shot_hook("needs"), overlay: false },
        Scenario {
            name: "needs_many",
            build: |s| {
                let tools: Vec<String> = ["cargo", "cmake", "pkg-config", "libwayland-dev", "libxkbcommon-dev", "protoc"].map(String::from).to_vec();
                for n in ["ForzaTelemetryV3Launcher", "ts6tray", "geofinder", "ritz", "ccorral"] {
                    s.badges.insert(n.into(), Badge::Needs(tools.clone()));
                }
                s.last_run = Some(LastRun { time: "t".into(), updated: 0, skipped: 0, failed: 2, failed_names: vec!["geofinder".into(), "ts6tray".into()] });
                s.selected = Some("Helpers/ts6tray".into());
            },
            overlay: false,
        },
        Scenario { name: "collapsed", build: |s| { s.collapsed.insert("Software".into()); s.collapsed.insert("Websites".into()); }, overlay: false },
    ];

    /// The helper-level guarantee: deliberately oversized content (a long unwrapped label, a
    /// 2000 px wide painted block) inside `theme::columns` + `theme::card` never paints outside its
    /// own column, and every card frame is exactly its column wide.
    #[test]
    fn oversized_content_stays_inside_its_pane() {
        for w in [800.0, 1100.0, 1235.0] {
            let ctx = context();
            let mut col_rects = Vec::new();
            let (out, tex) = run(&ctx, w, 400.0, |ui| {
                col_rects.clear();
                ui.spacing_mut().item_spacing.x = 8.0;
                crate::theme::columns(ui, 3, |cols| {
                    for ui in cols.iter_mut() {
                        col_rects.push(ui.max_rect());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        crate::theme::card(ui, "Card", |ui| {
                            ui.add(egui::Label::new("a very long label that would never fit into a narrow column at all").extend());
                            let (r, _) = ui.allocate_exact_size(egui::vec2(2000.0, 20.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 0.0, crate::theme::ACCENT);
                        });
                        // The next card is unaffected by the previous card's overflow, and
                        // its own rows fit.
                        crate::theme::card(ui, "Next card", |ui| {
                            crate::theme::row_label(ui, "A settings row label that is rather long for a column");
                            ui.add(crate::theme::secondary_button("Change\u{2026}"));
                        });
                    }
                });
            });
            snapshot(&ctx, &out, &tex, w as u32, 400, &format!("oversized_{w}"));
            assert_eq!(col_rects.len(), 3);
            for r in visible_rects(&out) {
                assert!(
                    col_rects.iter().any(|c| r.left() >= c.left() - 4.5 && r.right() <= c.right() + 4.5),
                    "at {w} px a shape paints across columns: {r:?} (columns {col_rects:?})"
                );
            }
        }
    }

    /// The whole app frame (top bar, optional bars, groups | side or stacked, log, status bar) in
    /// every scenario of [`SCENARIOS`] at 1280, 900 and 600 px: the panes never paint outside
    /// their pane (the strict rule, on `draw_panes`). Overlay scenarios (modals, the open `...`
    /// popover) additionally draw something on top: that must exist and stay inside the window,
    /// and a scenario with no overlay must draw nothing beyond the panes.
    #[test]
    fn whole_app_stays_inside_its_panes() {
        for sc in SCENARIOS {
          for ppp in SCALES {
            for (w, h) in SIZES {
                let name = if ppp == 1.0 { format!("app_{}_{}", sc.name, w as u32) } else { format!("app_{}_{}@{ppp}", sc.name, w as u32) };
                let make = || {
                    let mut s = full();
                    (sc.build)(&mut s);
                    // The log stamps lines with the wall clock: pin it, so the two renders compared
                    // below cannot differ by a second ticking over.
                    for l in &mut s.log {
                        l.time = "12:00:00".into();
                    }
                    s
                };
                // Baseline: the panes alone (no modal; popover closed), strict rule.
                let mut base_state = make();
                base_state.open_menu = None;
                let (_, base, _, base_rects) = render_panes_at(&base_state, w, h, ppp);
                assert_inside_panes(&base, &base_rects, w, h, &name);

                // The real frame: panes + overlays.
                let state = make();
                let (ctx, out, tex, rects) = render_app_at(&state, w, h, ppp);
                snapshot(&ctx, &out, &tex, w as u32, h as u32, &name);
                assert_eq!(rects.stacked, state.view == View::Main && w < crate::theme::STACK_BELOW, "{name}: layout mode");
                // (The stacked layout has no side pane when there are no programs, see `body_stacked`.)
                let side = state.view == View::Main && !(rects.stacked && state.listing.sections.is_empty());
                assert_eq!(rects.side.is_some(), side, "{name}: side pane presence");
                assert_eq!(rects.banner.is_some(), state.ritz_get_missing, "{name}: banner");
                assert_eq!(rects.restart.is_some(), state.restart, "{name}: restart bar");
                let extra = overlay_shapes(&out, &base);
                if sc.overlay {
                    assert!(!extra.is_empty(), "{name}: expected an overlay (modal / popover) to draw");
                    let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h)).expand(1.0);
                    for c in &extra {
                        let r = c.shape.visual_bounding_rect().intersect(c.clip_rect);
                        if r.is_positive() {
                            assert!(window.contains_rect(r), "{name}: an overlay shape paints outside the window: {r:?}");
                        }
                    }
                } else {
                    assert!(extra.is_empty(), "{name}: {} shapes beyond the panes without an overlay: {:?}", extra.len(), extra.iter().map(|c| (c.clip_rect, &c.shape)).collect::<Vec<_>>());
                }
            }
          }
        }
    }

    /// The fluid card grid at every size and display scale: 3 / 2 / 2 columns at 1280 / 900 / 600
    /// (cards 270..=350 pt wide), the cards of a row equally wide (within a physical pixel or so),
    /// the row reaching the pane's right edge (only the solid scroll bar's gutter is left) unless
    /// the cards are capped at `MAX_CARD_W`, all in points, so the result must not depend on
    /// `pixels_per_point`.
    #[test]
    fn card_grid_fills_the_row_at_every_scale() {
        let mut state = full();
        state.selected = None;
        for ppp in SCALES {
            for ((w, h), want_cols) in SIZES.into_iter().zip([3usize, 2, 2]) {
                let (_, out, _, rects) = render_panes_at(&state, w, h, ppp);
                let cards: Vec<Rect> = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Rect(r)
                            if r.stroke.width == 1.0
                                && r.stroke.color == crate::theme::BORDER
                                && r.rect.width() > 200.0
                                && r.rect.height() > 100.0
                                && rects.groups.expand(1.0).contains_rect(r.rect) => Some(r.rect),
                        _ => None,
                    })
                    .collect();
                assert!(!cards.is_empty(), "no cards found @ {w} x{ppp}");
                let top = cards.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
                let row: Vec<Rect> = cards.iter().copied().filter(|r| (r.top() - top).abs() < 0.5).collect();
                let widths: Vec<f32> = row.iter().map(|r| r.width()).collect();
                eprintln!("grid @ {w} x{ppp}: groups {:.1} wide, {} cols, widths {widths:?}", rects.groups.width(), row.len());
                assert_eq!(row.len(), want_cols, "columns @ {w} x{ppp}: {widths:?}");
                let (lo, hi) = (widths.iter().cloned().fold(f32::MAX, f32::min), widths.iter().cloned().fold(0.0, f32::max));
                assert!(hi - lo <= 1.5 / ppp, "unequal card widths @ {w} x{ppp}: {widths:?}");
                let max_w = crate::ui::groups::MAX_CARD_W + 1.0 / ppp;
                assert!(hi <= max_w && lo >= 270.0 - 1.0 / ppp, "card width outside 270..=350 @ {w} x{ppp}: {widths:?}");
                let right = row.iter().map(|r| r.right()).fold(0.0, f32::max);
                let gap = rects.groups.right() - right;
                if hi < crate::ui::groups::MAX_CARD_W - 1.0 {
                    assert!((-0.5..=14.0).contains(&gap), "right-hand gap {gap} @ {w} x{ppp} (groups {:?}, cards {row:?})", rects.groups);
                }
                for pair in row.windows(2) {
                    let g = pair[1].left() - pair[0].right();
                    assert!((g - crate::theme::GAP).abs() <= 1.5 / ppp, "card gap {g} @ {w} x{ppp}");
                }
            }
        }
    }

    /// Few cards in a wide window (the `unlisted` fixture: at most 3 cards in a section): three
    /// columns of exactly `MAX_CARD_W`, left-aligned with empty space on the right, at every
    /// scale; and widening from 1500 to 2560 changes nothing any more.
    #[test]
    fn few_cards_stay_capped_in_a_wide_window() {
        let mut state = full();
        state.listing = fixture("unlisted.porcelain");
        state.selected = None;
        for ppp in SCALES {
            let mut seen: Vec<Vec<f32>> = Vec::new();
            for w in [1500.0_f32, 1920.0, 2560.0] {
                let (_, out, _, rects) = render_panes_at(&state, w, 1000.0, ppp);
                let mut cards: Vec<Rect> = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Rect(r)
                            if r.stroke.width == 1.0
                                && r.stroke.color == crate::theme::BORDER
                                && r.rect.width() > 200.0
                                && r.rect.height() > 100.0
                                && rects.groups.expand(1.0).contains_rect(r.rect) => Some(r.rect),
                        _ => None,
                    })
                    .collect();
                cards.sort_by(|a, b| a.left().total_cmp(&b.left()));
                let widths: Vec<f32> = cards.iter().map(|r| r.width()).collect();
                let lefts: std::collections::BTreeSet<i32> = cards.iter().map(|r| (r.left() * 4.0).round() as i32).collect();
                assert_eq!(lefts.len(), 3, "3 columns @ {w} x{ppp}: {widths:?}");
                for c in &widths {
                    assert!((c - crate::ui::groups::MAX_CARD_W).abs() <= 1.0 / ppp, "card {c} @ {w} x{ppp}");
                }
                let right = cards.iter().map(|r| r.right()).fold(0.0, f32::max);
                assert!(rects.groups.right() - right > 100.0, "empty space on the right @ {w} x{ppp}");
                seen.push(widths);
            }
            assert!(seen.windows(2).all(|p| p[0].len() == p[1].len()));
        }
    }

    /// Every entry of the fixture selected in turn (hero, pills, setup steps, actions, details),
    /// each with the nastiest session badge it can carry: the side panel and the cards stay inside
    /// their panes at all three widths.
    #[test]
    fn every_selected_entry_stays_inside_its_panes() {
        let ids: Vec<(String, String)> = full().listing.entries().map(|e| (e.id(), e.name.clone())).collect();
        for (id, name) in ids {
            for (w, h) in SIZES {
                let mut s = full();
                s.selected = Some(id.clone());
                s.dir = Some("/home/u/.local/share/ritz-get".into());
                s.badges.insert(name.clone(), Badge::Needs(["cargo", "cmake", "pkg-config", "libwayland-dev"].map(String::from).to_vec()));
                let (_, out, _, rects) = render_panes(&s, w, h);
                assert_inside_panes(&out, &rects, w, h, &format!("select {id} @ {w}"));
                s.badges.insert(name.clone(), Badge::SetupNeeded);
                let (_, out, _, rects) = render_panes(&s, w, h);
                assert_inside_panes(&out, &rects, w, h, &format!("select {id} (setup needed) @ {w}"));
            }
        }
    }

    /// Side-by-side geometry: at >= 760 px the side pane is exactly 300 px wide, 8 px right of the
    /// groups pane, and both reach the same bottom; stacked, the side pane is full width below groups.
    #[test]
    fn panes_have_the_specified_geometry() {
        let state = full();
        let (_, _, _, wide) = render_app(&state, 1280.0, 800.0);
        let side = wide.side.unwrap();
        assert!(!wide.stacked);
        assert!((side.width() - crate::theme::SIDE_W).abs() < 0.5, "side width {}", side.width());
        assert!((side.left() - wide.groups.right() - crate::theme::GAP).abs() < 0.5);
        assert!((side.bottom() - wide.groups.bottom()).abs() < 0.5);
        let (_, _, _, narrow) = render_app(&state, 600.0, 800.0);
        let side = narrow.side.unwrap();
        assert!(narrow.stacked);
        assert!((side.width() - narrow.groups.width()).abs() < 0.5);
        assert!(side.top() >= narrow.groups.bottom() + crate::theme::GAP - 0.5);
    }

    /// No programs (ritz-get missing / refused): stacked, there is no side frame at all (it used to
    /// shrink to an empty 16 px sliver); side by side the full-height blank pane stays.
    #[test]
    fn stacked_without_programs_has_no_side_pane() {
        let mut state = full();
        state.listing = Default::default();
        state.selected = None;
        state.ritz_get_missing = true;
        let (_, _, _, narrow) = render_app(&state, 600.0, 800.0);
        assert!(narrow.stacked && narrow.side.is_none(), "stacked side: {:?}", narrow.side);
        let (_, _, _, wide) = render_app(&state, 1280.0, 800.0);
        assert!(!wide.stacked && wide.side.is_some());
    }

    /// Mockup `.input { height: var(--ctl) }`: every FIELD-filled text input / combo is `CTRL_H`
    /// high (the Settings path fields); the top-bar search pill is the one 30 px exception.
    #[test]
    fn inputs_are_ctrl_h_high() {
        for (name, view) in [("main", View::Main), ("settings", View::Settings)] {
            let mut state = full();
            state.view = view;
            state.dir = Some("/home/u/.local/share/ritz-get".into());
            let (_, out, _, _) = render_app(&state, 1280.0, 800.0);
            let heights: Vec<f32> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Rect(r) if r.fill == crate::theme::FIELD && r.rect.width() > 100.0 && r.rect.height() < 31.0 => Some(r.rect.height()),
                    _ => None,
                })
                .collect();
            assert!(!heights.is_empty(), "{name}: no FIELD-filled inputs found");
            // The top-bar search field is the one deliberately taller input (SEARCH_H, on every page).
            let is_search = |h: &f32| (*h - crate::ui::topbar::SEARCH_H).abs() < 0.6;
            assert_eq!(heights.iter().filter(|h| is_search(h)).count(), 1, "{name}: search fields in {heights:?}");
            for h in heights.iter().filter(|h| !is_search(h)) {
                assert!((h - crate::theme::CTRL_H).abs() < 0.6, "{name}: input heights {heights:?}, want {}", crate::theme::CTRL_H);
            }
        }
    }

    /// The rendered top bar takes the form the plan promises: wide at 1280 and 900 (search pill in
    /// row 1, centred in the window, 460 wide at 1280), narrow at 600 (full-width pill below row 1).
    #[test]
    fn top_bar_forms_at_the_test_sizes() {
        let state = full();
        for (w, wide) in [(1280.0_f32, true), (900.0, true), (600.0, false)] {
            let (_, out, _, rects) = render_app(&state, w, 800.0);
            let search: Vec<Rect> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Rect(r) if r.fill == crate::theme::FIELD && r.rect.height() == crate::ui::topbar::SEARCH_H => Some(r.rect),
                    _ => None,
                })
                .collect();
            assert_eq!(search.len(), 1, "{w}: {search:?}");
            let s = search[0];
            // 10 px bar padding, then row 1.
            let row1_top = rects.top.top() + 10.0;
            if wide {
                assert!((s.top() - row1_top).abs() < 0.6, "{w}: search at {s:?}, row 1 at {row1_top}");
                assert!((s.center().x - w / 2.0).abs() < 0.6, "{w}: search centred, got {s:?}");
                if w == 1280.0 {
                    assert!((s.width() - 460.0).abs() < 0.6, "{w}: search {s:?}");
                }
                assert!((rects.top.height() - (10.0 + 30.0 + 8.0 + 36.0 + 1.0)).abs() < 0.6, "{w}: bar {}", rects.top.height());
            } else {
                assert!(s.top() > row1_top + 30.0, "{w}: search on its own row, got {s:?}");
                assert!((s.width() - (w - 16.0)).abs() < 0.6, "{w}: search {s:?}");
                assert!((rects.top.height() - (10.0 + 30.0 + 10.0 + 30.0 + 8.0 + 36.0 + 1.0)).abs() < 0.6, "{w}: bar {}", rects.top.height());
            }
        }
    }

    /// One frame of the whole app with `events`; returns what the panes asked for and the frame.
    fn click_frame(ctx: &Context, state: &AppState, w: f32, h: f32, events: Vec<egui::Event>) -> (Vec<crate::model::Action>, FullOutput) {
        let mut actions = Vec::new();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))), events, ..Default::default() };
        let out = ctx.run(input, |ctx| {
            super::super::draw(ctx, state, &mut actions);
        });
        (actions, out)
    }

    /// Centre of the (first) painted text `label`.
    fn text_centre(out: &FullOutput, label: &str) -> Pos2 {
        out.shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == label => Some(c.shape.visual_bounding_rect().center()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text {label:?} painted"))
    }

    /// Click `at` (move, press, release over three frames); all actions pushed meanwhile.
    fn click(ctx: &Context, state: &AppState, w: f32, h: f32, at: Pos2) -> Vec<crate::model::Action> {
        use egui::{Event, PointerButton};
        let btn = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut all = Vec::new();
        for ev in [vec![Event::PointerMoved(at)], vec![btn(true)], vec![btn(false)], vec![]] {
            all.extend(click_frame(ctx, state, w, h, ev).0);
        }
        all
    }

    /// Every top-bar control still does its job: tabs -> `SetFilter` (and back from Settings),
    /// Check for updates, Update all, gear, and the Sort button's choices -> `SetSort`.
    #[test]
    fn top_bar_controls_push_their_actions() {
        use crate::model::{Action, Filter, Sort};
        let (w, h) = (1280.0, 800.0);
        let mut state = full();
        let ctx = context();
        crate::icons::letters_only_for_this_thread();
        let mut out = click_frame(&ctx, &state, w, h, vec![]).1;
        for _ in 0..2 {
            out = click_frame(&ctx, &state, w, h, vec![]).1;
        }
        let acts = click(&ctx, &state, w, h, text_centre(&out, "Installed"));
        assert!(acts.contains(&Action::SetFilter(Filter::Installed)), "{acts:?}");
        assert!(click(&ctx, &state, w, h, text_centre(&out, "Check for updates")).contains(&Action::CheckForUpdates));
        assert!(click(&ctx, &state, w, h, text_centre(&out, "Update all")).contains(&Action::Update { name: None }));
        assert!(click(&ctx, &state, w, h, Pos2::new(w - 8.0 - 15.0, 10.0 + 15.0)).contains(&Action::ShowSettings));
        // Settings is open: a tab goes back to the main page.
        state.view = View::Settings;
        let acts = click(&ctx, &state, w, h, text_centre(&out, "Updates"));
        assert!(acts.contains(&Action::SetFilter(Filter::Updates)) && acts.contains(&Action::ShowMain), "{acts:?}");
        assert!(click(&ctx, &state, w, h, Pos2::new(w - 8.0 - 15.0, 10.0 + 15.0)).contains(&Action::ShowMain));
        state.view = View::Main;
        // Sort: open the choices, pick one.
        let ctx = context();
        let mut out = click_frame(&ctx, &state, w, h, vec![]).1;
        for _ in 0..2 {
            out = click_frame(&ctx, &state, w, h, vec![]).1;
        }
        let acts = click(&ctx, &state, w, h, text_centre(&out, "Sort"));
        assert!(acts.is_empty(), "opening the menu must not change the sort: {acts:?}");
        let out = click_frame(&ctx, &state, w, h, vec![]).1;
        let acts = click(&ctx, &state, w, h, text_centre(&out, "Updates first"));
        assert_eq!(acts, vec![Action::SetSort(Sort::UpdatesFirst)]);
    }

    /// The open Sort choices list all four orders inside the window (with `RITZ_GET_GUI_SNAPSHOT_DIR`
    /// set, also written as `topbar_sort_popup_<w>.png`).
    #[test]
    fn top_bar_sort_popup_lists_every_choice() {
        for w in [1280.0_f32, 600.0] {
            let ctx = context();
            let mut first = true;
            let state = full();
            let (out, tex) = run_ctx(&ctx, w, 500.0, |ctx| {
                if std::mem::take(&mut first) {
                    egui::Popup::open_id(ctx, egui::Id::new("topbar_sort").with("popup"));
                }
                super::super::draw(ctx, &state, &mut Vec::new());
            });
            for s in crate::model::Sort::ALL {
                let c = text_centre(&out, s.label());
                assert!(c.x > 0.0 && c.x < w && c.y > 0.0 && c.y < 500.0, "{} at {c:?}", s.label());
            }
            snapshot(&ctx, &out, &tex, w as u32, 500, &format!("topbar_sort_popup_{w}"));
        }
    }

    /// While an Update all run is hidden the capsule's Update all half reads "Updating N of M…" and
    /// reopens the dialog.
    #[test]
    fn top_bar_reopens_a_hidden_update_all() {
        use crate::model::Action;
        let mut state = full();
        state.apply_shot_hook("updateall-running");
        state.modal = None;
        let label = state.reopen_label().expect("hidden run has a label");
        let ctx = context();
        let (w, h) = (1280.0, 800.0);
        let mut out = click_frame(&ctx, &state, w, h, vec![]).1;
        for _ in 0..2 {
            out = click_frame(&ctx, &state, w, h, vec![]).1;
        }
        assert!(label.starts_with("Updating"), "{label}");
        let acts = click(&ctx, &state, w, h, text_centre(&out, &label));
        assert!(acts.contains(&Action::ShowModal(Modal::Run)), "{acts:?}");
    }

    /// A theme gallery (fonts per weight, the glyphs, every pill, buttons, card) in a narrow and a
    /// wide column: renders, and nothing leaves its column. With `RITZ_GET_GUI_SNAPSHOT_DIR` set this
    /// writes `gallery_<w>.png` to eyeball fonts and tokens.
    #[test]
    fn theme_gallery_stays_inside_its_pane() {
        use crate::theme::{self, glyph, PillKind};
        for w in [300.0, 700.0] {
            let ctx = context();
            let mut col = Rect::NOTHING;
            let (out, tex) = run(&ctx, w, 480.0, |ui| {
                theme::columns(ui, 1, |c| {
                    let ui = &mut c[0];
                    col = ui.max_rect();
                    ui.spacing_mut().item_spacing.y = 0.0;
                    theme::card(ui, "Fonts", |ui| {
                        ui.label(egui::RichText::new("Geist Regular 13 - Hamburgefonstiv").font(theme::regular(13.0)));
                        ui.label(egui::RichText::new("Geist Medium 13 - Hamburgefonstiv").font(theme::medium(13.0)));
                        ui.label(egui::RichText::new("Geist Bold 13 - Hamburgefonstiv").font(theme::bold(13.0)));
                        ui.label(egui::RichText::new("Geist Mono 12 - 2026-10-14 3f9e21a").font(theme::mono(12.0)));
                    });
                    theme::card(ui, "Glyphs", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for g in [
                                glyph::GEAR, glyph::GITHUB, glyph::EXT, glyph::DOWNLOAD, glyph::REFRESH, glyph::TRASH,
                                glyph::UNDO, glyph::FOLDER, glyph::PLUS, glyph::MINUS, glyph::BOOK, glyph::BUG,
                                glyph::CHECK, glyph::PAUSE, glyph::SEARCH, glyph::CHEVRON, glyph::CHEVRON_R,
                                glyph::TERMINAL, glyph::WARNING,
                            ] {
                                theme::icon_button(ui, g, 22.0);
                            }
                        });
                    });
                    theme::card(ui, "Pills", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                            for k in [
                                PillKind::Ok, PillKind::Update, PillKind::Missing, PillKind::Needs, PillKind::Web,
                                PillKind::Held, PillKind::Modified, PillKind::Setup, PillKind::Offline,
                                PillKind::FetchFailed, PillKind::BranchGone, PillKind::SetupNeeded,
                            ] {
                                theme::pill(ui, k.label(), k.color());
                            }
                            theme::pill_glyph(ui, glyph::PAUSE, theme::TEXT_DIM);
                        });
                    });
                    theme::card(ui, "Controls", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(theme::primary_button("Update all (3)"));
                            ui.add(theme::secondary_button("Check for updates"));
                            ui.add(theme::danger_button("Discard and reset"));
                        });
                    });
                });
            });
            snapshot(&ctx, &out, &tex, w as u32, 480, &format!("gallery_{w}"));
            for r in visible_rects(&out) {
                assert!(
                    r.left() >= col.left() - 4.5 && r.right() <= col.right() + 4.5,
                    "at {w} px a gallery shape paints outside its column: {r:?} (column {col:?})"
                );
            }
        }
    }
}

