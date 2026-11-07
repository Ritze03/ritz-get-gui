# Program icons — local file, cached remote fetch, letter fallback

Each card, the side-panel hero and the progress dialogs show the program's own icon. `src/icons.rs` decides where the picture comes from, downloads it when the program is not installed, resamples it crisply to the display size, and falls back to a coloured letter tile.

## How it works

**Choosing a source (`icons.rs:icon_source`)**

`IconSource` has three variants, decided from the entry and the clone folder `dir` (from `ritz-get dir`, `None` until known). Only a cheap `Path::exists` touches the disk.

| Entry | Source |
| --- | --- |
| installed or unlisted, `<dir>/<name>/icon.png` exists | `File` of that path |
| installed or unlisted, no such file or `dir` unknown | `Letter` (no remote fallback for installed programs) |
| website, `<dir>/ritz-get/icons/<name>.png` exists | `File` of that path |
| website otherwise | `Remote` from the ritz-get repo's `icons/<name>.png` on its default branch (the constant `SITE_ICONS`; hardcoded, marked with a `ponytail:` comment) |
| not installed with a parseable GitHub `source` (`OWNER/REPO` or a github.com URL, `parse_owner_repo`) | `Remote`: `raw.githubusercontent.com/OWNER/REPO/<branch or HEAD>/icon.png` |
| anything else | `Letter` |

The `Remote` cache file is `~/.cache/ritz-get-gui/icons/<owner>_<repo>_<branch>.png` (`cache_dir`, honours `$XDG_CACHE_HOME`; names pass through `sanitize`). Website icons are cached as `<owner>_<repo>_HEAD_icon_<name>.png`. The branch is the manifest branch of the not-installed entry, `HEAD` when empty.

**The letter fallback**

`letter` makes `IconSource::Letter { ch, color }`: the first character of `Entry::display()` uppercased, on a rounded square whose colour comes from a deterministic FNV-1a hash of `Entry::name` into a five-colour palette (`letter_color`). `draw` paints the tile inset by 1/16 of the size, because the real artwork's tile sits inside its canvas, with the text at half the icon size.

**Fetching and the sha sidecar**

- `IconFetcher` (owned by `App`, `src/app.rs`) is asked by `App::request_icons` for every entry after each parsed listing and after the `dir` query returns. `IconFetcher::request` ignores anything that is not `Remote`.
- Each `(cache_path, latest sha)` pair is requested once per session (`requested` set); repeat frames and repeat listings do nothing. When `latest` has a new sha it is a new key and may fetch again.
- Next to every cached PNG sits `<name>.sha` (`sidecar`, `with_extension("sha")`) holding the latest sha the icon was fetched for. `should_fetch` is the pure decision: no sidecar means fetch; a sidecar equal to the sha with the PNG present means skip; a different sha, or a missing PNG, means fetch. A sidecar `!<sha>` records a failed fetch (404 or network) at that sha, so the next launch does not retry until the sha changes. *Why:* the module comment says this stops the app hammering the network every launch.
- Website icons have no sha (empty string), so a failure writes no sidecar: the retry happens on the next launch, because a `!` marker would block retries forever (comment in `fetch_to_cache`).
- The download runs on its own thread with a 10 s timeout (`ureq`), accepts only bytes starting with the PNG signature, writes to a `.tmp` file and renames it. On success it clears the path from the failure set, drops egui's cached image and the resampled copies (`ctx.forget_image`, `forget_scaled`) and requests a repaint.

**Process-wide state, so `draw` needs no fetcher**

`icons::draw(ui, entry, dir, size)` is a free function called from `groups.rs` (48 pt on cards), `side.rs` (88 pt hero) and `modals.rs` (progress dialog rows). It has no handle to the app, so shared state lives in `static` `OnceLock<Mutex<...>>` values:

- `failed()`: cache paths whose fetch failed this session. `draw` treats a `Remote` as ready only when the file exists and is not in this set.
- `last_good()`: the last image drawn per `Entry::id`. `resolve` falls back to it when the source yields no image, so uninstalling (which deletes the clone's `icon.png` before the remote icon is fetched) keeps the old picture instead of flashing a letter. Only an entry that never had an image shows the letter.
- `scaled()`: resampled textures keyed by `(file, device pixels)`; `sources()`: the PNG bytes of every file read so far. A new size (scroll, scale change) is resampled from the remembered bytes even if the file is gone.

**Smooth downscaling**

Icons are large PNGs drawn at a few dozen points. `scaled_texture` resamples on a background thread with Lanczos3 to exactly `size * pixels_per_point` device pixels and draws that texture 1:1 (`downscale`, `target_px`), rounded to the pixel grid. Resampling happens in premultiplied alpha and clamps ringing so the transparent margin leaves no dark fringe. *Why:* the module comment says letting the GPU minify a ~1024 px image about 28x with bilinear filtering skips most pixels and gives jagged edges. While a texture is being prepared `draw` paints nothing (a repaint follows); an undecodable file becomes a letter.

**Refreshing after changes**

`App::icons_to_refresh` (`src/app.rs`) lists the `icon.png` of clones a finished command may have changed (successful install, switch, reset, and every program an update reported as updated) and `forget_scaled` drops their cached bytes and textures so the next draw re-reads the file. Never remove: the last-good icon must survive an uninstall.

**The app's own icon**

`own_texture(ctx, size)` resamples the embedded repo-root `icon.png` (`include_bytes!`) for the top bar brand mark, cached per device size.

**Tests**

`icons::letters_only_for_this_thread` (test builds) makes `draw` paint only letter tiles on that thread, so layout tests do not depend on the process-wide caches or resampling timing.

## Using it

Nothing to configure. Installed programs show the `icon.png` from their clone; programs not installed show their repo's `icon.png` once downloaded (a letter until then); a program without an icon keeps its letter tile.

## Options

| Item | Value | Meaning |
| --- | --- | --- |
| `$XDG_CACHE_HOME` | falls back to `~/.cache` | Base of the icon cache `ritz-get-gui/icons` |

## Related links

- [program-list.md](program-list.md) (cards that draw icons)
- [../progress-dialog/run-dialog.md](../progress-dialog/run-dialog.md) (dialog rows reuse `icons::draw`)
- [../ritz-get-bridge/command-runner.md](../ritz-get-bridge/command-runner.md) (`dir` query that provides the clone folder)
- [../../architecture/overview.md](../../architecture/overview.md)
