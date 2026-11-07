# Run dialog — the progress dialog and per-card bars

Every install, update, repair, switch, reset, remove and delete-data (and Update all) opens one dialog, `Modal::Run`, and puts a progress bar on the affected card(s). Its content is `AppState::run`; the cards' bars are `AppState::progress`. The log behind it is in [log.md](log.md), the non-happy endings in [error-flows.md](error-flows.md).

## How it works

State (all in `src/app.rs`):

- `Run` — one run: `cmd`, `verb`, `name` (`None` = Update all), `order` (Update all's programs in CLI order), `phase`, `stopping`, `started`, `settling`, `ask_menu`, `change` (a single update's `A -> B`), and a private `log_start`: the absolute index of the run's `$ ritz-get ...` log line, so `AppState::run_lines` is a slice of the one shared log even after the log cap drops old lines.
- `RunPhase` — `Running`, `Done { menu }`, `Failed { reason }`, `Deps { missing, command }` (exit 3), `Setup` (exit 4), `Busy` (exit 5), `Stopped`.
- `opens_run(cmd)` decides which commands open it: install, update, switch, reset, remove, delete-data. `tracked(cmd)` (a different, smaller set: install, update, switch, reset) decides which get card bars. Remove and delete-data have a dialog but no bar.
- `Progress` — a card's state: `Queued`, `Running`, `Done`, `Result(Outcome)` (the `NAME: ...` result line), `Failed`. There is no percentage: the CLI prints none, so a running bar is an indeterminate sweep. `CardProgress` adds the `Verb` and `ended` (the fade clock); `CardProgress::tone` / `label` give colour and text ("Queued", "Updating…", "Updated", "Up to date", "Skipped", "FAILED").
- `Marks` — what ritz-get's `#start NAME` / `#progress NAME N/M` machine lines said (see [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md)). `AppState::progress_start` / `progress_count` feed it from `App::handle_event`; machine lines are never logged. `base` accumulates earlier segments because ritz-get re-executes itself after updating itself and restarts its count. `legacy` is set when a result only an apply step can print arrives before any `#start`: this ritz-get prints no markers, so `promote_progress` keeps inferring the running card from result lines.

Lifecycle:

1. `App::next_to_start` pushes the `$ ritz-get ...` line, then `AppState::begin_progress` (bars) and `begin_run` (dialog, `Modal::Run`). A single command's card starts `Running`; Update all queues every `updatable()` entry without a pending branch and promotes the first. Pending branch switches run first as their own `switch` commands (`App::dispatch`).
2. Streamed lines: `classify` colours them, `progress_result` turns `NAME: ...` result lines into card states.
3. `App::finished` calls `finish_progress` (settles cards with no result line: `Done` on success, `Failed` for a single command that failed; Update all's never-reached cards just disappear), then `mutation_finished` → `AppState::finish_run`.
4. `finish_run`: exit 0 does not end the dialog yet. It sets `settling` (and `change`) and waits for the refreshed online listing; `settle_run` (called from `list_finished` for non-offline lists) then moves to `Done { menu: run.ask_menu }`. *Why:* the card's own button and the start-menu question both need the refreshed listing ("the card's own button must be right when it comes back", `expire_progress`). Exit 1/2/other become `Failed` (reason: first `FAILED` result, else the last red log line, else "it failed" / "usage error (exit 2)" / "exited with code N"); 3 is filled in by `deps_needed`; 4 `Setup`; 5 `Busy`; stop `Stopped`; a spawn failure drops the run.
5. `after_run_phase`: a result that needs the user (anything but a plain success without the menu question, and not `Stopped`) opens or reopens the dialog if it was hidden or already showing; a plain success while hidden just clears `run`.

*Why:* nothing for `Stopped`: "The user asked for it; nothing to tell them unless the dialog is open" (comment in `after_run_phase`).

### Run in background and the way back

While `Running`, Esc / backdrop / "Run in background" send `Action::CloseModal`. `apply_ui_action` closes the modal and keeps `run` (only a finished run is dismissed: `run = None`). `run_hidden()` is `run.is_some() && modal != Run`. Then:

- the card's bar becomes clickable (`src/ui/groups.rs`, `src/ui/side.rs` pass `run_hidden()` as `back`) and selects the card and reopens the dialog;
- the status bar (`src/ui/bottom.rs:draw_status`) puts `AppState::reopen_label` first, in accent colour, linking to `Modal::Run`: "Installing NAME…", "Updating 3 of 5…", "NAME: add to the start menu?", "NAME: install failed", "Missing tools", "Setup needed", "ritz-get is busy", "NAME: install stopped", "Update finished" / "Update failed" / "Update stopped" for Update all;
- Update all additionally turns the top bar's Update all half into the same label (`src/ui/topbar.rs`, only when `run.is_all()`).

The status bar's remaining items (checked-ago, offline, counts, "last update failed (see log)", Log, report link) and their drop order at narrow widths are documented at the top of `src/ui/bottom.rs`. It is a single row; the old log drawer above it is gone.

### Single run layout (`run_one`, 460 px)

- Running: 56 px icon, title "Installing X…" (`run_title`), mono sub-line (`sub_line`: `sha → sha` for update, `branch B` otherwise), sweep bar, one fixed-height status line (`AppState::run_status`: only lines ritz-get itself prints, i.e. whose prefix is a known program or a `missing:`/`deps:`/`note:`/`restart:`/`setup:`/`needs root:` hint, because install.sh's build output arrives on the same stream unmarked), optional log box, `[Show log] [Copy log]` on the left, `[Stop]` and `[Run in background]` on the right. Stop sets `Run::stopping` and calls `runner.stop()` (`Action::StopRun`, only for the run's own in-flight command).
- Done: green check, the newest `Ok` log line in a box, `[Done]`. After a remove that left data, `[Delete data…]` appears (it only opens `Modal::DeleteData`, see [error-flows](error-flows.md)) and the sub-line says "Data left: …".
- Done + `menu`: the "Add it to the start menu?" card with a menu-entry preview, `[Not now] [Add to start menu]` (Add has focus and sends `Action::Shortcut { remove: false }`).
- Failed: red alert box with the reason, a note (`failed_note`: "Nothing was installed…", "Your installed version is unchanged and still works."), `[Report on GitHub]` (programs with a repo; builds an issue URL from the log tail, capped by `ISSUE_URL_MAX`), `[Open full log]` (`Action::OpenRunLog`, see [log.md](log.md)), `[Show log]`, `[Close]`.
- Deps / Setup / Busy / Stopped: see [error-flows.md](error-flows.md).

### The start-menu question

Asked only after a **successful first install**. `mutation_finished` sets `App::offer_shortcut = Some(name)` only for `Command::Install` with exit ok where the program was **not** installed before (the same command on an installed program is `Verb::Repair`). The next online listing (`list_read`; an offline listing does not take or settle it) calls `offer_start_menu`, which sets `run.ask_menu` only when the entry is a non-website, installed, and its shortcut column is `ShortcutState::Off` (field 17, `on|off|-`). The pending offer is consumed by that online listing whether or not it works: if the `list` fails or does not parse, `ask_menu` stays false and the question is never asked. A refused listing (`state.refused`) never asks either.

### Update all (`run_all`, 500 px)

Title and count come from `run_all_title` / `run_counts` / `run_counted`: "Checking for updates…" until the first `#start`, then "Updating NAME…" until the first `#progress`, then "Updating N of M…" using ritz-get's own count (an older ritz-get without markers: the count of the set). A count bar (sweep while not counted), one row per program (`update_row`: icon, name, mono line, pill, own 3 px bar; the running row is tinted and scrolled into view once), and, when finished, a tally (`tally`: "2 updated, 1 up to date, …") and a note that failed programs' installed versions are unchanged. The icon is the app's own (`ritz-get-gui`).

### Bar fade

When a command ends, `finish_progress` stamps `ended`. `expire_progress` (called every frame from `App::update`) removes finished bars `PROGRESS_FADE` (2000 ms) later, but not while `busy`, `loading`, or the run dialog is open (`Modal::Run` with a run): the dialog's rows *are* these bars and go once it closes. It returns the time to the next expiry for the repaint timer. `progress_running` keeps a repaint timer going while a bar animates.

### Reset failure keeps MODIFIED

A failed reset logs a note ("failed: see the FAILED lines above") and the normal refresh re-lists the program, which therefore still shows MODIFIED (`mutation_finished`, `ExitKind::Failed` branch, `src/app.rs:1833`). Nothing clears local-changes state on failure.

## Using it

Click Install / Update / Repair / Switch / Reset / Remove or Update all. The dialog opens. Leave it with "Run in background" and come back by clicking the card's bar, the status bar's accent item, or (Update all) the top bar button. Stop cancels the command (a running build is killed; what was already done stays). After a first install, answer the start-menu question or "Not now".

## Related links

- [log.md](log.md), [error-flows.md](error-flows.md)
- [command-runner](../ritz-get-bridge/command-runner.md), [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md)
- [program-actions](../programs/program-actions.md), [program-list](../programs/program-list.md)
- [testing-and-screenshots](../packaging/testing-and-screenshots.md) (the `progress`, `progress-single=`, `startmenu=` screenshot hooks)
- [architecture overview](../../architecture/overview.md), [terminology](../../meta/TERMINOLOGY.md)
