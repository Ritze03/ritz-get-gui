# Command runner — runs one `ritz-get` CLI call at a time and streams it to the UI

The GUI never touches programs itself: every install, update, listing or setting change is a `ritz-get` subprocess. `src/runner.rs` spawns it on a worker thread, streams its lines to the UI thread, and reports the exit code plus a `Captured` bundle of the lines worth parsing. The command vocabulary (`Command`, `ExitKind`) lives in `src/model.rs`.

## How it works

- **`Command`** (`src/model.rs:Command`) is one CLI invocation: `List`, `Install`, `Update`, `Remove`, `Switch`, `Reset`, `DeleteData`, `Hold`, `Unhold`, `Open`, `Shortcut`, `Shortcuts`, `Dir`, `Branches`. UI panes emit `Action`s; `src/model.rs:Action::to_command` maps them (`Repair` is `Install` without a branch; `CheckForUpdates` is `List { offline: false, all: false, data: true }`).
- **`Command::args`** is the single source of truth for argv after the binary name. Notable shapes: `list [--offline] --porcelain [--all] [--data]`; `update [NAME] --yes`; `reset NAME --yes`; `delete-data NAME --yes`; `dir PATH --yes` (bare `dir` takes none); `shortcuts [PATH]` (never `--yes`); `install NAME [--branch B]`; `switch NAME BRANCH`.
- **`--yes` policy:** `update`, `reset`, `delete-data` and `dir PATH` always carry `--yes`, because stdin is never a terminal, so the CLI could not ask. The comment in `Command::args` quotes PORCELAIN.md: "Always pass `--yes` to `update`, `reset` and `dir PATH`".
- **`Command::is_query`:** `List`, `Branches`, and `Dir`/`Shortcuts` without a PATH. Their stdout is data, so the runner puts it in `Captured::stdout` and emits no `RunnerEvent::Line` for stdout (stderr lines are still streamed). `Dir { path: Some(_) }` is not a query.
- **Environment and stdio** (`src/runner.rs:run_command`):
  - `RITZ_GET_GUI=1` on every command (overrides a parent value of `0`). It makes ritz-get print the `#start` / `#progress` marker lines.
  - stdin is `/dev/null` (`Stdio::null()`), stdout and stderr are piped, and the child leads its own process group (`process_group(0)`).
  - `RITZ_GET_SUDO=pkexec` is set only when `Command::uses_sudo` is true: `Install`, `Remove`, `Switch`, `Reset`, `Update` (including Update all, `name: None`). For every other command the runner calls `env_remove("RITZ_GET_SUDO")`, so a value in the GUI's own environment cannot leak into them. `DeleteData` never gets it (doc comment on its variant, test `command_args` in `src/model.rs`).
  - *Why:* `Update` got pkexec in commit 6d90aa7 because install.sh only asks for root when it is actually needed (a missing system link), so a normal update stays silent, and ritz-get honours `RITZ_GET_SUDO` on update. A graphical password prompt then appears only in that rare case.
- **Binary:** `Runner::binary` returns `$RITZ_GET` if set and non-empty, else `ritz-get` from `PATH`. Tests use `Runner::start_with(bin, ..)` to run against `tests/fake-ritz-get` or small shell scripts.
- **Spawn failure:** one stderr `Line` (`<bin>: <os error>`), then `Finished { exit: ExitKind::SpawnFailed }`. This drives the "ritz-get not found" banner (`src/app.rs` sets `ritz_get_missing`).
- **`ExitKind::from_code`** (`src/model.rs`): `0` Ok, `1` Failed, `2` Usage, `3` Deps (tools missing), `4` NeedsRoot, `5` Busy (another ritz-get holds the lock), any other code `Other(n)`, killed by a signal (`None`) `Other(-1)`. `SpawnFailed` and `Stopped` do not come from codes: the first from a failed spawn, the second from `Runner::stop` (a non-Ok result after Stop becomes `Stopped`; Ok stays Ok). For multi-program runs ritz-get already reports the highest-priority code (3 over 4 over 1).
- **One at a time:** `Runner::start` returns `false` and does nothing while a command runs. `Runner::poll` drains `RunnerEvent`s non-blockingly; after `Finished` the runner is idle. If the worker thread panics (channel disconnected) `poll` synthesises `Finished` with `ExitKind::Other(-1)` so the app is not busy forever. Every thread-side line calls `ctx.request_repaint()`.
- **Pipe handling:** lines are split on `\n`, trailing `\r`/`\n` stripped, bytes decoded lossily (`from_utf8_lossy`). After the child exits the runner keeps draining for `DRAIN_GRACE` (300 ms), because a detached grandchild (for example `xdg-open`) may hold the pipes open forever.
- **`Captured::absorb`** (every line, both streams; stdout also accumulates into `Captured::stdout`):
  - `missing: TOOL (try: ...)` goes to `missing`, kept once per distinct text.
  - `deps: CMD` sets `deps` (last one wins). `None` means unknown distro, so the UI shows the raw `missing` lines.
  - `needs root: ...` goes to `needs_root`, deduplicated (ritz-get repeats these at the end of a run).
  - `N updated, N skipped, N failed` sets `summary` (`RunSummary`).
  - `NAME: up to date | updated A -> B | held (...) | SKIPPED (local changes...) | SKIPPED (needs a newer ritz-get...) | FAILED (reason)` pushes a `ProgramResult` with an `Outcome`. Other `NAME: ...` lines (shortcut created, needs repair) are not results.
- **Markers:** `parse_marker` turns `#start NAME` into `Marker::Start` and `#progress NAME N/M` into `Marker::Progress`; it rejects empty or whitespace-containing names, `M == 0`, non-numeric fractions, and any other `#...` line such as `#porcelain 1`. `is_machine_line` is true for `#` followed by a non-whitespace char: the log view and Copy log skip these, while `# comment` from a build script is kept as text. Marker lines are not results (`absorb` ignores them) but stay in `Captured::stdout`. See [run-dialog](../progress-dialog/run-dialog.md) and [log](../progress-dialog/log.md) for how the UI uses them.
- **Stop** (`Runner::stop`): marks the run stopped, sends SIGTERM to ritz-get alone (its traps stop the build and clean up), waits `STOP_GRACE` (5 s) on a helper thread, then SIGKILLs the whole process group and every descendant found by walking `/proc` (`descendants`; builds started through `timeout` leave the group). If Stop is pressed before the spawn finished, the worker sends SIGTERM as soon as it has a pid. When the child exits after Stop, the waiter also SIGKILLs the group's leftovers.
  - *Why killing is lock-safe:* the flock on fd 9 dies with the process, so SIGKILL cannot leave a stale lock (doc comment of `Runner::stop`, `src/runner.rs:290-296`).
- **`branches(output)`** parses `ritz-get branches NAME` output: one branch per line, `* ` prefix marks the current one, blank lines skipped; returns `(branch, is_current)` in order.
- **Old-CLI fallback for `--data`** (`src/app.rs`, handling of a finished `List { data: true }`): an old ritz-get that does not know `--data` exits 2 (`ExitKind::Usage`). The app then sets `no_data_flag`, rewrites every queued `List` to `data: false`, re-queues this list without `--data`, and `list_cmd` (`data: !no_data_flag`) builds all later lists. `D` records (see [porcelain-parsing](porcelain-parsing.md)) therefore only exist with a new enough ritz-get.

## Open questions

- The original reason `RITZ_GET_SUDO` is removed for every command other than install/remove/switch/reset/update (instead of being left alone) is not stated in the repo. The code comment only says "Never inherit it from our own environment". Ask the author before recording a rationale.
- `D<n>` / `I<n>` tags in comments (for example D48 on `uses_sudo`, I3 on the runner) refer to an external plan that is not in the repo; their meaning is unknown here.

## Tests

`src/runner.rs` tests cover env policy (the parent `RITZ_GET_SUDO` and `RITZ_GET_GUI` are poisoned on purpose), argv, closed stdin, exit mapping, Stop killing the group, odd output, detached grandchildren, and the fake CLI scenarios. See [testing-and-screenshots](../packaging/testing-and-screenshots.md) for the fake CLI.

## Related links

- [porcelain-parsing](porcelain-parsing.md) — what `list --porcelain` stdout becomes
- [run-dialog](../progress-dialog/run-dialog.md), [log](../progress-dialog/log.md), [error-flows](../progress-dialog/error-flows.md) — consumers of `RunnerEvent`, `Captured`, `ExitKind`
- [program-actions](../programs/program-actions.md) — which UI actions produce which `Command`
- [Architecture overview](../../architecture/overview.md)
