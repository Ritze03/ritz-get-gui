# Program list — sections, cards, filters, pills and the top bar

The main page: every program and website from `ritz-get list --porcelain` as a card grouped by manifest section, narrowed by a filter tab, a search string and a sort order, with status pills on each card and a top bar that holds the tabs, counts, Check for updates and Update all (N).

## How it works

**What is shown (`src/app.rs`, slice of `AppState`)**

- `AppState::visible_sections` returns, per `Section` in manifest order (`Unlisted` last), the entries that pass `state != Unreachable`, `Filter::matches` and the search, sorted per `Sort`. Sections that end up empty are dropped, so a header never shows `0`.
  - Search: `search.trim().to_lowercase()` is a substring test against `Entry::display()`, `Entry::name` and `Entry::desc`. An empty query matches everything.
  - `Sort::Default` keeps manifest order. `Name` sorts by lowercase `display()`. `UpdatesFirst` sorts by `!has_update()`, `InstalledFirst` by `!is_installed()`. Both are stable sorts, so ties keep manifest order. Sorting is per section; sections never reorder.
- `AppState::counts` fills `Counts { all, installed, updates, not_installed }` (`src/app.rs:Counts`) from the whole listing, ignoring search and the active filter. Only `Unreachable` entries are skipped. The tab badges read these numbers, so they do not change while you type.
- `Filter::matches` (`src/model.rs:Filter`): `All` is everything; `Installed` is `is_installed()` (state `installed` or `unlisted`); `Updates` is `has_update()`; `NotInstalled` is a non-website that is not installed. So websites appear under All only, and an Unlisted clone counts as Installed. `has_update()` is true when upstream is ahead *or* the user picked another branch (`pending_branch`), so a picked branch moves a program into Updates and the Update all count.
- `unreachable` and `#hidden` entries: ritz-get omits them unless called with `--all`, and the app always sends `all: false` (`src/app.rs:App::list_cmd`). `visible_sections` and `counts` still filter `State::Unreachable` defensively (test `unreachable_entries_are_hidden_and_uncounted`). `Listing::hidden` (the `#hidden N` line) is parsed but nothing renders it. *Why:* the file's own comments say hidden programs are "not rendered or counted"; they do not give a deeper reason.
- Filter, search, sort and group collapse are plain `Action`s (`SetFilter`, `SetSearch`, `SetSort`, `ToggleGroup`) applied by `AppState::apply_ui_action`, which handles every UI-only action and hands everything else back (`Some(other)`) to `App::dispatch` for the command path. `SetSort` is also saved to the settings file in `App::dispatch` before it is applied (see [../settings/settings.md](../settings/settings.md)). Selecting a card (`Action::Select`) sets `scroll_to_side` and `scroll_to_card` only when the selection changed, so a click never re-centres an already visible card.

**The grid (`src/ui/groups.rs`)**

- `groups::draw` calls `visible_sections`; with none it paints `empty`, whose text depends on state: ritz-get missing, listing refused, first `list` still loading, nothing loaded, or "No programs match".
- One column count is used for all sections (`grid_columns` / `grid_widths`), capped by the longest section so a few cards do not create empty columns. Cards are `MIN_CARD_W` to `MAX_CARD_W` wide. Column edges snap to whole physical pixels (`snap`) so neighbouring 1 pt borders never leave seams at fractional display scales.
- Cards in a grid row share the tallest card's height: `measure` runs the same `content` function invisibly (`paint = false`), then `card` paints. The primary button row is pinned to the bottom.
- A card is: pills and a `↗` Open on GitHub button (not for Unlisted entries, not when `repo_url()` is `None`), icon plus name plus two-line description, a version line (`version_job`), then the primary button and the `...` menu. Clicking the card body selects it; the side panel then shows that entry.
- Version line (`version_job`): website shows `short_host(url)` (host, last path segment; scheme and extension dropped). Installed with upstream ahead: `<installed> installed → <latest> latest`. Installed otherwise: `<installed> · branch <b>` plus ` → <pending>` when a branch is picked. Not installed: `latest <sha>`.
- The primary button and the `...` popover are covered in [program-actions.md](program-actions.md). Icons: [icons.md](icons.md).

**Pills (`src/ui/pills.rs:pills`)**

One pure function `pills(state, entry) -> Vec<Pill>` serves both the cards and the side-panel hero, so the two never disagree. Order and conditions:

| Condition | Pill |
| --- | --- |
| website | Website (accent) |
| installed or unlisted, `status=fetch-failed` | Fetch failed (danger) |
| installed, `status=branch-gone` | Branch deleted (warn); tooltip from `branch_gone_text` |
| installed, `has_update()` | Update available (warn) |
| installed, `status=current` | Up to date (good) |
| installed, any other status (`unknown`, `none`, modified-only) | **no status pill** |
| not installed, session `Badge::Needs(tools)` | one `Needs <tool>` pill per tool; replaces Not installed / Offline |
| not installed, `state=offline` | Offline |
| not installed otherwise | Not installed |
| not installed, has `D` data records | Data left (tooltip = `data_label()`) |
| session `Badge::SetupNeeded` | Setup needed |
| `held` | pause glyph only, tooltip "Auto-update off (held)" |
| `status=modified` | Modified (tooltip = changed-file count) |
| entry has `S` setup steps | Setup |
| `AppState::last_failed(e)` (name is in the last update run's `failed:` line) | Failed (last) |

- An installed entry also gets the Needs pills after its status pill (they do not replace it).
- Offline is never shown for an installed entry: installed + `unknown` shows nothing, and the version line carries the information. *Why:* the test comment says the side hero "used to show it for `unknown`".
- Modified-only gets no status pill because `status=modified` wins the status column; the separate Modified pill covers it. `has_update()` can still be true for a modified program (it keeps its `behind` count), so such a card shows Update available *and* Modified.
- Unlisted entries use the installed branch of the mapping (they are `is_installed()`), so they show the same status pills.
- `pills::show` paints one pill, `pills::width` measures it for the hero's centred wrapping (`side.rs:pill_rows`). Colours and labels live in `theme::PillKind` ([../../ui/STYLING-GUIDE.md](../../ui/STYLING-GUIDE.md)).

**Top bar (`src/ui/topbar.rs`)**

- Layout is computed once per frame by `plan(&Metrics, origin)` and every item is painted into its own rect, so nothing can overflow its pane. Two forms: wide (search pill centred in the *window*, capsule on the right) and narrow (search on its own row, Check for updates and Sort reduced to icons). Narrow is chosen when the centred search would be under `SEARCH_MIN_CENTRED` (150 px).
- Row 1: brand icon (`icons::own_texture`) and wordmark, search pill, the capsule `[Check for updates | Update all (N)]`, gear. Row 2: four underline tabs (All, Installed, Updates, Not installed) with count badges, and the ghost Sort button with a popup of the four `Sort` choices.
- Check for updates: enabled when `actions_enabled()` and not `loading`; label becomes "Checking…" while loading. Click pushes `Action::CheckForUpdates`.
- Update all (N): N is `AppState::update_all_count()` = entries with `updatable()` (has update, not held, not modified). Enabled when N > 0 and `actions_enabled()`; muted otherwise. Tooltip when `update_all_skipped()` > 0: "N held or modified update(s) skipped". While an Update all run is in the background (`state.run.is_all()` and `reopen_label()` is `Some`) this half becomes a "show the update again" button that pushes `ShowModal(Modal::Run)` (see [../progress-dialog/run-dialog.md](../progress-dialog/run-dialog.md)).
- A tab count in Updates turns warn-coloured when above zero. In the Settings view no tab is selected and clicking one pushes `SetFilter` plus `ShowMain`.
- Search buffer: the edit text lives in egui memory as `(buffer, last pushed value)` and re-syncs when `state.search` changes behind its back.
- Restart bar: `topbar::draw_restart`, drawn by `ui::draw` only when `state.restart` (set after a successful self-update; see [../ritz-get-bridge/command-runner.md](../ritz-get-bridge/command-runner.md)). Message "ritz-get-gui was updated. Restart to use the new version." and a primary Restart button pushing `Action::Restart`, which re-execs the installed binary (`App::restart_self`).
- `CheckForUpdates` in `App::dispatch` runs `list --porcelain --data` and then re-asks `branches NAME` for every program whose branch list is already cached. *Why:* the in-code comment says branch lists are fetched once per program, so a new remote branch would otherwise never appear.

## Using it

1. Pick a tab (All / Installed / Updates / Not installed); the badge numbers are totals, not affected by search.
2. Type in the search pill to narrow by title, name or description.
3. Use Sort to reorder inside each section.
4. Click a section header to collapse or expand it.
5. Click a card to select it and open its details in the side panel; use its button or `...` for actions ([program-actions.md](program-actions.md)).
6. Check for updates re-reads the listing; Update all (N) updates every non-held, non-modified program that has an update.

## Related links

- [program-actions.md](program-actions.md), [icons.md](icons.md)
- [../settings/settings.md](../settings/settings.md) (sort persistence)
- [../ritz-get-bridge/porcelain-parsing.md](../ritz-get-bridge/porcelain-parsing.md) (where `Entry`, `State`, `Status` come from)
- [../progress-dialog/run-dialog.md](../progress-dialog/run-dialog.md) (what Update all opens)
- [../../architecture/overview.md](../../architecture/overview.md), [../../ui/STYLING-GUIDE.md](../../ui/STYLING-GUIDE.md)
