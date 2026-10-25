//! Program icons (**I6**).
//!
//! Icon loader: clone or ritz-get `icons/` file, ureq fetch to the disk cache with a sha sidecar,
//! coloured letter fallback. (D13, D59)
//!
//! Contract: the UI panes (I9 cards, I10 side panel hero) only call [`draw`]; the app (I7) owns an
//! [`IconFetcher`] and calls [`IconFetcher::request`] for entries that have a
//! [`IconSource::Remote`] source after each listing; images go through `egui_extras` loaders as
//! `file://` URIs (the loaders are installed in `App::new`). Fetch/failure state is shared through
//! a process-wide static, so the free [`draw`] needs no fetcher.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use egui::Color32;

use crate::model::Entry;
use crate::theme;

/// Where an entry's icon comes from (D13, D59).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSource {
    /// A PNG on disk: installed programs `<dir>/<name>/icon.png`, websites `<dir>/ritz-get/icons/<name>.png`.
    File(PathBuf),
    /// Not installed: fetch `url` (`raw.githubusercontent.com/OWNER/REPO/BRANCH|HEAD/icon.png`) once
    /// into `cache_path` (`~/.cache/ritz-get-gui/icons/<owner>_<repo>_<branch>.png`, sidecar = latest sha).
    Remote { url: String, cache_path: PathBuf },
    /// No icon: a coloured letter square.
    Letter { ch: char, color: Color32 },
}

/// Parse `OWNER/REPO` out of an `OWNER/REPO` or `https://github.com/OWNER/REPO[.git]` source.
fn parse_owner_repo(src: &str) -> Option<(String, String)> {
    let s = src.trim().trim_end_matches('/').trim_end_matches(".git");
    let s = if let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) {
        rest.strip_prefix("github.com/").or_else(|| rest.strip_prefix("www.github.com/"))?
    } else {
        s
    };
    let mut it = s.split('/');
    let (owner, repo) = (it.next()?, it.next()?);
    if it.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }
    let ok = |p: &str| p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (ok(owner) && ok(repo)).then(|| (owner.to_owned(), repo.to_owned()))
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') { c } else { '-' }).collect()
}

/// Deterministic tile colour from the name (FNV-1a into a small Graphite palette).
fn letter_color(name: &str) -> Color32 {
    let palette = [theme::ACCENT, theme::GOOD, theme::WARN, theme::steel(150), theme::steel(190)];
    let mut h: u32 = 0x811c9dc5;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x01000193);
    }
    palette[h as usize % palette.len()]
}

fn letter(entry: &Entry) -> IconSource {
    IconSource::Letter {
        ch: entry.display().chars().next().unwrap_or('?').to_uppercase().next().unwrap_or('?'),
        color: letter_color(&entry.name),
    }
}

/// Decide the icon source for `entry`. `dir` is the clone folder from `ritz-get dir` (`None` until known).
/// Only a cheap `Path::exists` touches the disk.
pub fn icon_source(entry: &Entry, dir: Option<&Path>) -> IconSource {
    let file = if entry.is_website() {
        dir.map(|d| d.join("ritz-get").join("icons").join(format!("{}.png", entry.name)))
    } else if entry.is_installed() {
        dir.map(|d| d.join(&entry.name).join("icon.png"))
    } else {
        None
    };
    if let Some(f) = file {
        return if f.exists() { IconSource::File(f) } else { letter(entry) };
    }
    if !entry.is_website() && !entry.is_installed() {
        if let Some((owner, repo)) = entry.source.as_deref().and_then(parse_owner_repo) {
            let branch = entry.branch.as_deref().filter(|b| !b.is_empty());
            let rf = branch.unwrap_or("HEAD");
            return IconSource::Remote {
                url: format!("https://raw.githubusercontent.com/{owner}/{repo}/{rf}/icon.png"),
                cache_path: cache_dir().join(format!("{}_{}_{}.png", sanitize(&owner), sanitize(&repo), sanitize(rf))),
            };
        }
    }
    letter(entry)
}

/// `~/.cache/ritz-get-gui/icons` (`$XDG_CACHE_HOME` honoured).
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("ritz-get-gui").join("icons")
}

// ---- sha sidecar ------------------------------------------------------------------------------
// `<cache>.sha` holds the latest sha the icon was fetched for; `!<sha>` = a failed fetch (404/network)
// at that sha, so we don't hammer the network every launch until the sha changes.

fn sidecar(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("sha")
}

/// Pure refetch decision. `sidecar` = sidecar contents (if any), `png_exists` = cached PNG on disk.
fn should_fetch(sidecar: Option<&str>, png_exists: bool, latest_sha: &str) -> bool {
    match sidecar.map(str::trim) {
        None => true,
        Some(s) => match s.strip_prefix('!') {
            Some(failed_at) => failed_at != latest_sha,
            None => s != latest_sha || !png_exists,
        },
    }
}

// ---- fetcher ----------------------------------------------------------------------------------

/// Cache paths whose fetch failed this session (shared so the free [`draw`] needs no fetcher).
fn failed() -> &'static Mutex<HashSet<PathBuf>> {
    static F: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    F.get_or_init(Default::default)
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let bytes = agent.get(url).call().ok()?.into_body().read_to_vec().ok()?;
    bytes.starts_with(&[0x89, b'P', b'N', b'G']).then_some(bytes)
}

fn fetch_to_cache(url: &str, cache_path: &Path, sha: &str) -> bool {
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match fetch(url) {
        Some(bytes) => {
            let tmp = cache_path.with_extension("tmp");
            if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, cache_path).is_ok() {
                let _ = std::fs::write(sidecar(cache_path), sha);
                return true;
            }
            false
        }
        None => {
            let _ = std::fs::remove_file(cache_path);
            let _ = std::fs::write(sidecar(cache_path), format!("!{sha}"));
            false
        }
    }
}

/// Background fetcher for remote icons (`ureq` on a thread, D59).
#[derive(Default)]
pub struct IconFetcher {
    requested: HashSet<(PathBuf, String)>,
}

impl IconFetcher {
    pub fn new() -> IconFetcher {
        IconFetcher::default()
    }

    /// Make sure `entry`'s remote icon is (being) fetched; calls `ctx.request_repaint()` when a
    /// download lands. No-op for non-remote sources or when the cache is current for the latest sha.
    pub fn request(&mut self, entry: &Entry, dir: Option<&Path>, ctx: &egui::Context) {
        let IconSource::Remote { url, cache_path } = icon_source(entry, dir) else { return };
        let sha = entry.latest.as_ref().map(|v| v.sha.clone()).unwrap_or_default();
        if !self.requested.insert((cache_path.clone(), sha.clone())) {
            return;
        }
        let side = std::fs::read_to_string(sidecar(&cache_path)).ok();
        if !should_fetch(side.as_deref(), cache_path.exists(), &sha) {
            return;
        }
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let uri = format!("file://{}", cache_path.display());
            if fetch_to_cache(&url, &cache_path, &sha) {
                failed().lock().unwrap().remove(&cache_path);
                // a re-downloaded file at the same path must not be served from egui's stale cache
                ctx.forget_image(&uri);
                forget_scaled(&cache_path);
            } else {
                failed().lock().unwrap().insert(cache_path);
            }
            ctx.request_repaint();
        });
    }
}

/// Last image drawn per entry id (`section/name`), process-wide. Uninstalling deletes the clone
/// (installed `File` source) and the not-installed `Remote` icon isn't fetched yet, so for ~1 s the
/// plain source is a letter; this keeps showing the previous image meanwhile (egui keeps the
/// decoded texture of an already-shown `file://` URI even after the file is deleted).
fn last_good() -> &'static Mutex<HashMap<String, PathBuf>> {
    static L: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

/// What to paint for `entry`: [`icon_source`] with `Remote` resolved to its cache file once
/// `ready(cache_path)`; whenever that yields no image (clone gone, remote not fetched / failed) the
/// entry's previous image from `last` is used; only without one the letter. Records the image shown.
fn resolve(entry: &Entry, dir: Option<&Path>, last: &mut HashMap<String, PathBuf>, ready: impl Fn(&Path) -> bool) -> IconSource {
    let file = match icon_source(entry, dir) {
        IconSource::File(p) => Some(p),
        IconSource::Remote { cache_path, .. } if ready(&cache_path) => Some(cache_path),
        _ => None,
    };
    match file {
        Some(p) => {
            if last.get(&entry.id()) != Some(&p) {
                last.insert(entry.id(), p.clone());
            }
            IconSource::File(p)
        }
        None => last.get(&entry.id()).map_or_else(|| letter(entry), |p| IconSource::File(p.clone())),
    }
}

// ---- smooth downscaling ------------------------------------------------------------------------
// Real icons are 1024 px PNGs drawn at ~36 / 88 points. Uploading them as-is and letting the GPU
// minify ~28x with plain bilinear skips most source pixels (jagged, "pixelated" edges). So each icon
// is resampled on the CPU with a Lanczos filter to exactly the device pixels it covers
// (`size * pixels_per_point`) and that small texture is drawn 1:1.

/// One resampled icon: still being prepared, ready, or undecodable.
enum Slot {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

/// Resampled textures per (file, device pixel size): a ppp or size change makes a new key.
fn scaled() -> &'static Mutex<HashMap<(PathBuf, u32), Slot>> {
    static S: OnceLock<Mutex<HashMap<(PathBuf, u32), Slot>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// PNG bytes of every icon file read successfully so far (a handful of ~100 KB files, not the
/// decoded 4 MB bitmaps). A new device size (card scrolled into view, ppp change) is resampled
/// from these, so an uninstall that deleted the clone cannot turn the icon into a letter.
fn sources() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<u8>>>> {
    static S: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// The PNG bytes of `path`: freshly read from disk (and remembered), else the remembered copy.
fn source_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => {
            let b = Arc::new(b);
            sources().lock().unwrap().insert(path.to_path_buf(), b.clone());
            Some(b)
        }
        Err(_) => sources().lock().unwrap().get(path).cloned(),
    }
}

/// `path` resampled to `px`, from disk or (file gone) from the remembered bytes.
fn scaled_image(path: &Path, px: u32) -> Option<egui::ColorImage> {
    downscale(&source_bytes(path)?, px)
}

/// Device pixels an icon of `size` points covers.
fn target_px(size: f32, ppp: f32) -> u32 {
    ((size * ppp).round() as u32).max(1)
}

/// Decode a PNG and resample it to `px` x `px` with Lanczos3. Resampled in premultiplied alpha so
/// the transparent margin of the artwork does not bleed dark fringes into the rounded edge.
fn downscale(bytes: &[u8], px: u32) -> Option<egui::ColorImage> {
    let mut img = image::load_from_memory(bytes).ok()?.into_rgba8();
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        for c in &mut p.0[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let mut out = if img.dimensions() == (px, px) {
        img
    } else {
        image::imageops::resize(&img, px, px, image::imageops::FilterType::Lanczos3)
    };
    for p in out.pixels_mut() {
        // Lanczos ringing can push a premultiplied channel above alpha, which is invalid
        let a = p[3];
        for c in &mut p.0[..3] {
            *c = (*c).min(a);
        }
    }
    Some(egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], out.as_raw()))
}

/// The resampled texture for `path` at `px`, starting the (background) work on first use.
/// `Ok(None)` while loading, `Err` for an undecodable file (the caller falls back to the letter).
fn scaled_texture(ctx: &egui::Context, path: &Path, px: u32) -> Result<Option<egui::TextureHandle>, ()> {
    let key = (path.to_path_buf(), px);
    let mut map = scaled().lock().unwrap();
    match map.get(&key) {
        Some(Slot::Ready(t)) => return Ok(Some(t.clone())),
        Some(Slot::Loading) => return Ok(None),
        Some(Slot::Failed) => return Err(()),
        None => {}
    }
    map.insert(key.clone(), Slot::Loading);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let slot = match scaled_image(&key.0, key.1) {
            Some(img) => Slot::Ready(ctx.load_texture(
                format!("icon:{}@{}", key.0.display(), key.1),
                img,
                egui::TextureOptions::LINEAR,
            )),
            None => Slot::Failed,
        };
        scaled().lock().unwrap().insert(key, slot);
        ctx.request_repaint();
    });
    Ok(None)
}

/// Drop the resampled textures and the remembered bytes of `path` (its file was re-downloaded or
/// its clone was reinstalled / updated); the next draw re-reads the file.
pub fn forget_scaled(path: &Path) {
    sources().lock().unwrap().remove(path);
    scaled().lock().unwrap().retain(|(p, _), _| p != path);
}

#[cfg(test)]
thread_local! {
    /// Layout tests render on their own thread and must not depend on the process-wide icon caches
    /// (`last_good`, `scaled`) that parallel tests fill, nor on the resampling thread's timing: with
    /// this set, `draw` paints the letter tile only.
    static LETTERS_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test helper: make `draw` on this thread paint letter tiles only (see `LETTERS_ONLY`).
#[cfg(test)]
pub fn letters_only_for_this_thread() {
    LETTERS_ONLY.with(|c| c.set(true));
}

/// Draw `entry`'s icon in a `size` x `size` square at the cursor (36 on cards, 88 in the side
/// panel hero). Paints only inside that square. Remote icons show once the fetcher has cached them.
pub fn draw(ui: &mut egui::Ui, entry: &Entry, dir: Option<&Path>, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let mut src = resolve(entry, dir, &mut last_good().lock().unwrap(), |p| p.exists() && !failed().lock().unwrap().contains(p));
    #[cfg(test)]
    if LETTERS_ONLY.with(std::cell::Cell::get) {
        src = letter(entry);
    }
    if let IconSource::File(path) = &src {
        // a 1:1 texture only stays crisp when it sits on the pixel grid
        let rect = egui::emath::GuiRounding::round_to_pixels(rect, ui.ctx().pixels_per_point());
        let px = target_px(rect.width(), ui.ctx().pixels_per_point());
        match scaled_texture(ui.ctx(), path, px) {
            Ok(Some(tex)) => {
                egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), rect.size()))
                    .fit_to_exact_size(rect.size())
                    .corner_radius(egui::CornerRadius::same((size * 0.23) as u8))
                    .paint_at(ui, rect);
                return resp;
            }
            Ok(None) => return resp, // being resampled; repaints when done
            Err(()) => src = letter(entry),
        }
    }
    if let IconSource::Letter { ch, color } = src {
        // the artwork's tile sits 32/512 inside its canvas
        let r = rect.shrink(size * 0.0625);
        ui.painter().rect_filled(r, egui::CornerRadius::same((r.width() * 0.23) as u8), color);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ch.to_string(), theme::bold(size * 0.5), theme::PRIMARY_TEXT);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, ShortcutState, State, Status, Version};

    fn entry(name: &str, kind: Kind, state: State, source: Option<&str>) -> Entry {
        Entry {
            section: "S".into(),
            name: name.into(),
            title: None,
            kind,
            source: source.map(Into::into),
            state,
            status: Status::None,
            held: false,
            branch: None,
            installed: None,
            latest: Some(Version { date: None, sha: "abc1234".into() }),
            behind: None,
            dirty: None,
            ahead: None,
            desc: String::new(),
            shortcut: ShortcutState::None,
            setup: vec![],
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("icons-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_owner_repo() {
        assert_eq!(parse_owner_repo("Ritze03/ritz"), Some(("Ritze03".into(), "ritz".into())));
        assert_eq!(parse_owner_repo("https://github.com/a/b.git"), Some(("a".into(), "b".into())));
        assert_eq!(parse_owner_repo("https://github.com/a/b/"), Some(("a".into(), "b".into())));
        assert_eq!(parse_owner_repo("https://example.com/a/b"), None);
        assert_eq!(parse_owner_repo("noslash"), None);
        assert_eq!(parse_owner_repo("a/b/c"), None);
    }

    #[test]
    fn installed_and_website_files() {
        let d = tmp("files");
        std::fs::create_dir_all(d.join("ritz")).unwrap();
        std::fs::create_dir_all(d.join("ritz-get/icons")).unwrap();
        let p = entry("ritz", Kind::Program, State::Installed, Some("o/ritz"));
        assert!(matches!(icon_source(&p, Some(&d)), IconSource::Letter { .. }));
        std::fs::write(d.join("ritz/icon.png"), b"x").unwrap();
        assert_eq!(icon_source(&p, Some(&d)), IconSource::File(d.join("ritz/icon.png")));
        let u = entry("ritz", Kind::Program, State::Unlisted, None);
        assert_eq!(icon_source(&u, Some(&d)), IconSource::File(d.join("ritz/icon.png")));
        let w = entry("site", Kind::Website, State::Website, Some("https://x.y"));
        assert!(matches!(icon_source(&w, Some(&d)), IconSource::Letter { .. }));
        std::fs::write(d.join("ritz-get/icons/site.png"), b"x").unwrap();
        assert_eq!(icon_source(&w, Some(&d)), IconSource::File(d.join("ritz-get/icons/site.png")));
        assert!(matches!(icon_source(&p, None), IconSource::Letter { .. }));
    }

    #[test]
    fn remote_for_not_installed() {
        let mut e = entry("ritz", Kind::Program, State::Available, Some("Ritze03/ritz"));
        match icon_source(&e, None) {
            IconSource::Remote { url, cache_path } => {
                assert_eq!(url, "https://raw.githubusercontent.com/Ritze03/ritz/HEAD/icon.png");
                assert_eq!(cache_path.file_name().unwrap(), "Ritze03_ritz_HEAD.png");
            }
            o => panic!("{o:?}"),
        }
        e.branch = Some("feat/x".into());
        match icon_source(&e, None) {
            IconSource::Remote { url, cache_path } => {
                assert!(url.ends_with("/Ritze03/ritz/feat/x/icon.png"));
                assert_eq!(cache_path.file_name().unwrap(), "Ritze03_ritz_feat-x.png");
            }
            o => panic!("{o:?}"),
        }
        e.source = None;
        assert!(matches!(icon_source(&e, None), IconSource::Letter { .. }));
    }

    #[test]
    fn sha_sidecar_decision() {
        assert!(should_fetch(None, false, "a"));
        assert!(should_fetch(None, true, "a"));
        assert!(!should_fetch(Some("a\n"), true, "a"));
        assert!(should_fetch(Some("a"), true, "b"));
        assert!(should_fetch(Some("a"), false, "a"));
        assert!(!should_fetch(Some("!a"), false, "a"));
        assert!(should_fetch(Some("!a"), false, "b"));
    }

    #[test]
    fn icon_survives_install_then_remove() {
        let d = tmp("remove");
        std::fs::create_dir_all(d.join("ritz")).unwrap();
        std::fs::write(d.join("ritz/icon.png"), b"x").unwrap();
        let mut last = HashMap::new();
        let none_ready = |_: &Path| false;
        let installed = entry("ritz", Kind::Program, State::Installed, Some("Ritze03/ritz"));
        let clone_icon = d.join("ritz/icon.png");
        assert_eq!(resolve(&installed, Some(&d), &mut last, none_ready), IconSource::File(clone_icon.clone()));
        // `ritz-get remove`: the clone is gone but the listing still says installed ...
        std::fs::remove_dir_all(d.join("ritz")).unwrap();
        assert!(matches!(icon_source(&installed, Some(&d)), IconSource::Letter { .. }));
        assert_eq!(resolve(&installed, Some(&d), &mut last, none_ready), IconSource::File(clone_icon.clone()));
        // ... then the listing says not installed and the remote icon isn't fetched yet.
        let gone = entry("ritz", Kind::Program, State::Available, Some("Ritze03/ritz"));
        assert!(matches!(icon_source(&gone, Some(&d)), IconSource::Remote { .. }));
        assert_eq!(resolve(&gone, Some(&d), &mut last, none_ready), IconSource::File(clone_icon));
        // The fetched remote icon takes over, and is what stays afterwards.
        let IconSource::Remote { cache_path, .. } = icon_source(&gone, Some(&d)) else { panic!() };
        assert_eq!(resolve(&gone, Some(&d), &mut last, |p| p == cache_path), IconSource::File(cache_path.clone()));
        assert_eq!(resolve(&gone, Some(&d), &mut last, none_ready), IconSource::File(cache_path));
    }

    #[test]
    fn letter_without_a_previous_image() {
        let mut last = HashMap::new();
        let gone = entry("ritz", Kind::Program, State::Available, Some("Ritze03/ritz"));
        assert!(matches!(resolve(&gone, None, &mut last, |_| false), IconSource::Letter { .. }));
        // another entry's image is not borrowed
        last.insert("S/other".into(), PathBuf::from("/x.png"));
        assert!(matches!(resolve(&gone, None, &mut last, |_| false), IconSource::Letter { .. }));
        let site = entry("site", Kind::Website, State::Website, Some("https://x.y"));
        assert!(matches!(resolve(&site, None, &mut last, |_| false), IconSource::Letter { .. }));
    }

    #[test]
    fn target_px_follows_size_and_ppp() {
        assert_eq!(target_px(36.0, 1.0), 36);
        assert_eq!(target_px(36.0, 1.5), 54);
        assert_eq!(target_px(88.0, 1.25), 110);
        assert_eq!(target_px(36.0, 2.0), 72);
        assert_ne!(target_px(36.0, 1.0), target_px(36.0, 1.5)); // other ppp = other cache key
        assert_eq!(target_px(0.0, 1.0), 1);
    }

    /// A 256 px PNG: opaque white disc (r=96) on a fully transparent black background.
    fn disc_png() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(256, 256, |x, y| {
            let (dx, dy) = (x as f32 - 127.5, y as f32 - 127.5);
            if dx * dx + dy * dy <= 96.0 * 96.0 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([0, 0, 0, 0]) }
        });
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn downscale_is_exact_size_and_has_no_dark_fringe() {
        let out = downscale(&disc_png(), 36).unwrap();
        assert_eq!(out.size, [36, 36]);
        let mut soft = 0;
        for p in &out.pixels {
            // premultiplied white: every channel equals alpha (a dark fringe would give rgb < a)
            assert!(p.r() <= p.a() && p.g() <= p.a() && p.b() <= p.a());
            assert!(p.a().abs_diff(p.r()) <= 2, "{p:?}");
            if p.a() > 8 && p.a() < 247 {
                soft += 1;
            }
        }
        assert!(soft > 20, "edge should be antialiased, {soft} partial pixels");
        assert_eq!(out.pixels[0].a(), 0); // corner stays transparent
        assert_eq!(out.pixels[18 * 36 + 18].a(), 255); // centre stays opaque
        assert!(downscale(b"not a png", 36).is_none());
    }

    /// Poll `scaled_texture` until the background thread has finished: `Some(Ok)` / `Some(Err)`.
    fn wait_texture(ctx: &egui::Context, path: &Path, px: u32) -> Result<egui::TextureHandle, ()> {
        for _ in 0..400 {
            match scaled_texture(ctx, path, px) {
                Ok(Some(t)) => return Ok(t),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(()) => return Err(()),
            }
        }
        panic!("texture never became ready");
    }

    #[test]
    fn icon_resamples_to_a_new_size_after_its_file_is_deleted() {
        let dir = tmp("deleted-resample");
        let path = dir.join("icon.png");
        std::fs::write(&path, disc_png()).unwrap();
        let ctx = egui::Context::default();
        let a = wait_texture(&ctx, &path, 36).expect("size A");
        assert_eq!(a.size(), [36, 36]);
        std::fs::remove_file(&path).unwrap(); // `ritz-get remove` deleted the clone
        let b = wait_texture(&ctx, &path, 54).expect("size B must come from the remembered bytes");
        assert_eq!(b.size(), [54, 54]);
        // a never-read missing file is still Failed
        assert!(wait_texture(&ctx, &dir.join("other.png"), 36).is_err());
        // a re-download drops the remembered bytes: the missing file now fails at a new size
        forget_scaled(&path);
        assert!(wait_texture(&ctx, &path, 72).is_err());
    }

    /// A changed icon.png shows after `forget_scaled`, with pixels of the new file.
    #[test]
    fn changed_file_shows_new_pixels_after_forget() {
        let dir = tmp("changed-icon");
        let path = dir.join("icon.png");
        let solid = |c: [u8; 4]| {
            let img = image::RgbaImage::from_pixel(64, 64, image::Rgba(c));
            let mut out = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
            out
        };
        std::fs::write(&path, solid([255, 0, 0, 255])).unwrap();
        let ctx = egui::Context::default();
        assert_eq!(wait_texture(&ctx, &path, 32).unwrap().size(), [32, 32]);
        assert_eq!(scaled_image(&path, 32).unwrap().pixels[0].r(), 255);
        std::fs::write(&path, solid([0, 0, 255, 255])).unwrap();
        // without forgetting, the cached texture is the old one (the bug)
        let old = wait_texture(&ctx, &path, 32).unwrap();
        forget_scaled(&path);
        let new = wait_texture(&ctx, &path, 32).unwrap();
        assert_ne!(old.id(), new.id(), "a fresh texture was made");
        let img = scaled_image(&path, 32).unwrap();
        assert_eq!((img.pixels[0].r(), img.pixels[0].b()), (0, 255), "pixels come from the new file");
    }

    #[test]
    fn letter_is_deterministic() {
        assert_eq!(letter_color("ritz"), letter_color("ritz"));
        let e = entry("ritz", Kind::Program, State::Available, None);
        match icon_source(&e, None) {
            IconSource::Letter { ch, color } => {
                assert_eq!(ch, 'R');
                assert_eq!(color, letter_color("ritz"));
            }
            o => panic!("{o:?}"),
        }
    }
}
