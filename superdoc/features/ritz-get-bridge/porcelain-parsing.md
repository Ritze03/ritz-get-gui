# Porcelain parsing — turns `ritz-get list --porcelain` into a `Listing`

The program list comes from `ritz-get list --porcelain`, a stable line format for frontends (specified in ritz-get's `PORCELAIN.md`, which is not in this repo). All parsing lives in `src/porcelain.rs:parse`; the result types are in `src/model.rs`.

## How it works

- **Input:** the complete stdout of the list command (`Captured::stdout`, see [command-runner](command-runner.md)). Fields are TAB-separated; CRLF and blank lines are tolerated.
- **First line:** `#porcelain N` must be the first non-blank line, else `PorcelainError::MissingHeader`. `N != 1` returns `PorcelainError::UnsupportedVersion(N)` before anything else is read; a non-numeric `N` is a `BadLine`.
- **Directives** (`#key arg`, anywhere after the header): `#online 1|0` sets `Listing::online` (`Some(bool)`, `None` if absent; any other value is a `BadLine`); `#hidden N` sets `Listing::hidden` (0 if absent; non-numeric is a `BadLine`). Unknown `#...` lines and `# comments` are ignored.
- **Records** (first field):
  - `E`: one program or website (`Entry`). Needs at least 16 fields (the `E` tag plus 15 data columns); the 17th field is the `shortcut` column. Fewer than 16 is a `BadLine { line, message }` with the 1-based line number. Fields beyond 17 are ignored. 16 fields (older ritz-get) gives `ShortcutState::None`.
  - `S NAME TEXT`: a manual setup step (`SetupStep`), attached to the preceding `E`; exactly 3 fields, and `NAME` must match that `E`, else `BadLine`. A stray `S` is also a `BadLine`.
  - `D NAME PATH KB`: a declared data folder that exists (`DataPath`, only with `list --porcelain --data`); exactly 4 fields, same attach rules as `S`. A `KB` that does not parse (ritz-get sends `-` when `du` failed) becomes `None`.
  - Any other record type is ignored. *Why:* so records added by a newer ritz-get do not break older GUIs (comment in `src/porcelain.rs:parse`).
- **Sections:** `Section`s keep first-seen (manifest) order; the section named `Unlisted` (`Listing::UNLISTED`, installed clones not in the manifest) is sorted last. Unlisted entries have `State::Unlisted`, source `-` (`None`) and no `S` lines.
- **Entry columns** (table in the header of `src/model.rs`): section, name, title (`None` if empty or equal to `name`; use `Entry::display`), `Kind` (`program`/`website`, anything else is a `BadLine`), source (`-` is `None`), `State`, `Status`, held (`1`/`0`, else `BadLine`), branch, installed, latest, behind, dirty, ahead (`-` is `None`, else a number or `BadLine`), desc (may be empty), shortcut.
- **Enums and helpers in `src/model.rs`:**
  - `State`: `installed`, `available`, `unreachable` (only with `--all`), `offline` (not installed, network unknown), `website`, `unlisted`; anything else is `State::Other(String)`.
  - `Status` (meaningful for installed/unlisted, else `-` giving `Status::None`): `current`, `behind`, `modified` (wins over the rest), `fetch-failed`, `branch-gone`, `unknown` (installed but offline); else `Other`. `held` is not a status: it is `Entry::held`.
  - `ShortcutState`: `on`, `off`, anything else `None` (not installed, no `.desktop`, or the column is missing).
  - `Version`: `YYYY-MM-DD sha` (date and sha) or a bare sha (the `latest` of a not-installed program); `-` or empty is `None`.
- **`Entry` conveniences** read by the UI: `has_update`, `updatable` (what "Update all (N)" counts: update on offer, not held, not modified), `behind_upstream`, `is_installed` (installed or unlisted), `repo_url`, `website_url`, `data_label`. `Entry::pending_branch` is GUI-only state, never from porcelain.
- **Refusing an unknown version.** *Why:* the GUI "refuses to guess" about a format it does not know (README). When `parse` returns `UnsupportedVersion(n)`, `src/app.rs` sets `AppState::refused = Some(n)`, drops queued lists and logs "ritz-get speaks porcelain N, this ritz-get-gui only understands 1: update ritz-get-gui". `src/ui/modals.rs` (module doc, lines 32-34) then shows a non-dismissible "Your ritz-get is newer than this version ... (porcelain N)." dialog with an `Update ritz-get-gui` button, because the app has nothing to show until it is updated. `src/porcelain.rs:PorcelainError` documents the variant as "refuse and offer 'update ritz-get-gui'" (line 28).
- Other parse errors (`BadLine`, `MissingHeader`) only log "could not read the listing: ...".

## Fixtures and sample data

- `tests/fixtures/*.porcelain`: `full` (online, all cases, 19 entries), `offline`, `hidden` (`#hidden 2`), `unlisted`, `legacy-16` (16-field `E` lines, no shortcut column), `bad-version` (`#porcelain 2`, plus an extra future column). `src/porcelain.rs` tests include them with `include_str!`.
- `src/sample.rs:listing` is the same data as an in-code `Listing`; test `full_matches_sample_listing` asserts `parse(full.porcelain)` equals it, so the two must be edited together.
- `tests/fake-ritz-get` is a POSIX-sh fake CLI that prints these fixtures for `list` (it strips `D` lines unless `--data` is given, and with `FAKE_RITZ_GET_NO_DATA_FLAG` exits 2 like an old CLI). See [testing-and-screenshots](../packaging/testing-and-screenshots.md).

## Related links

- [command-runner](command-runner.md) — produces the stdout, and the `--data` fallback
- [program-list](../programs/program-list.md) — how a `Listing` is shown (sections, filters, sort)
- [program-actions](../programs/program-actions.md)
- [Architecture overview](../../architecture/overview.md)
