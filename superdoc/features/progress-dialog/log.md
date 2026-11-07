# Log — the shared log, its colours, and the log views

One in-memory log of everything the GUI ran, shown in two places: the progress dialog's "Show log" box (the current run only) and the whole-log dialog `Modal::Log`. There is no bottom log drawer any more (`src/ui/mod.rs`, `src/ui/bottom.rs`).

## How it works

- `AppState::log: Vec<LogLine>` (`LogLine { time, kind, text }`, `src/model.rs`). `AppState::push_log` stamps the local `HH:MM:SS` and keeps the newest `LOG_CAP` (5000) lines, counting the dropped ones in `log_dropped` so a `Run`'s absolute `log_start` stays valid ([run-dialog.md](run-dialog.md)).
- Every non-query command logs a `$ <command>` line (`LogKind::Cmd`) when it starts (`App::next_to_start`); the runner's streamed lines are pushed in `App::handle_event`. Machine lines (`#start`, `#progress`, any future `#...` on stdout) are consumed by the progress marks and never logged.
- `classify` (`src/app.rs`) picks the `LogKind` of a streamed line:
  - `Err` (red): contains `FAILED`, or starts (case-insensitive) with `error`, `fatal`, `busy:` or `missing:`;
  - `Note` (accent): starts with `needs root:`, `deps:` or `Additional steps`;
  - `Ok` (green): `NAME: updated A -> B` (`is_updated_line`), `shortcut created`, `shortcut removed`, or a `delete-data` result (`is_delete_data_line`: `data deleted`, `no data left`, `a website has no data`);
  - otherwise `Info`. `Cmd` lines are made by the app, not by `classify`.
  The GUI also pushes its own `Err` / `Note` lines (for example "`list` failed (exit N)", "`CMD` failed: see the FAILED lines above", "`CMD` was stopped", "ritz-get not found: install it, then press Retry").
- `src/ui/logview.rs:show` draws the log box: a FIELD box with mono lines coloured by `kind_color` (Info text, Cmd dim, Ok good, Err danger, Note accent), optional faint time column, wrapped lines, sticks to the bottom as lines arrive. Only visible lines are laid out; cumulative wrapped heights are cached in egui memory (`LineHeights`, keyed by `salt`) and recomputed when the width changes or the cap drops the head. `salt` keeps scroll position and cache apart per view (`run<log_start>` for a run, `full` for the log dialog). *Why:* per-line heights are cached because the log can hold thousands of wrapped lines (comment in the module header: "Only the visible lines are laid out").
- Progress dialog: `modals.rs:run_log` shows `AppState::run_lines()` limited to the newest `logview::RUN_LINES` (200), no time column. Toggled by `Action::ToggleRunLog` (`AppState::run_log_open`, reset to closed when a new run begins).
- `Modal::Log` (`modals.rs:log_modal`, 680 px): title "Log" with a line count, the whole log with the time column, `[Close]` and `[Copy log]`. Opened from the status bar's "Log" link and from "last update failed (see log)" (`bottom.rs`, `Link::Log` / `Link::Failed`).
- `Action::CopyLog` (`App::dispatch`) copies the **whole** log as `HH:MM:SS text` lines, even when pressed in the progress dialog.

### "Open full log" (`run_log_file`)

The failed-run dialog's `[Open full log]` (`Action::OpenRunLog`) opens a file with `xdg-open`. `App::run_log_file` picks it:

1. ritz-get's own `<cache>/ritz-get/logs/NAME.log` (all of install.sh's output) **if** its modification time is at or after `Run::started`;
2. otherwise the run's lines as the GUI captured them, written to `<cache>/ritz-get-gui/last-run.log`.

*Why:* "a failure before install.sh, e.g. while fetching the source, leaves an old or no build log" (doc comment). Update all has no single `NAME`, so it always uses the GUI's own capture. `<cache>` is derived from `last_run_path` (`$XDG_CACHE_HOME`, else `~/.cache`; see `last_run_file_from`); with no cache folder it logs "no cache folder to find the log in" and opens nothing. A write failure is logged as an `Err` line.

## Using it

Open the progress dialog's "Show log" to watch a run; open "Log" in the status bar for everything since launch. Copy log puts it on the clipboard.

## Related links

- [run-dialog.md](run-dialog.md), [error-flows.md](error-flows.md)
- [command-runner](../ritz-get-bridge/command-runner.md) (what streams in), [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md) (`#start` / `#progress`)
- [testing-and-screenshots](../packaging/testing-and-screenshots.md) (`log`, `logerr` shot hooks)
