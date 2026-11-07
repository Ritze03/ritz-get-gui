# Error flows — what the user sees when a command does not just succeed

How each ritz-get exit code, a missing binary, a refused porcelain version and the two confirmation dialogs (`Modal::Reset`, `Modal::DeleteData`) surface in the UI. The happy path and the dialog's lifecycle are in [run-dialog.md](run-dialog.md).

## How it works

Exit codes map in `ExitKind` (`src/model.rs`; `exit_code` in `src/app.rs` turns them back into numbers for log text). `App::mutation_finished` logs and, for 3 and 4, calls `deps_needed` / `setup_needed`; `AppState::finish_run` sets the dialog's `RunPhase`.

| Exit | Phase | Card badge | Dialog |
| --- | --- | --- | --- |
| 1 / other | `Failed { reason }` | none (the refresh decides) | red alert, [Report on GitHub], [Open full log] |
| 2 | `Failed` "usage error (exit 2)" | none | same; also an `Err` log line |
| 3 | `Deps { missing, command }` | `Badge::Needs(tools)` | "Missing tools" |
| 4 | `Setup` | `Badge::SetupNeeded` | "Needs setup in a terminal" |
| 5 | `Busy` | none (bars cleared) | "ritz-get is busy" |
| stop | `Stopped` | none | "Stopped …" |

### Exit 3: Missing tools

`App::deps_needed` collects tool names from the `missing: TOOL (try: ...)` lines (`tool_of_missing`) and from `FAILED (needs: a, b)` reasons (`tools_of_reason`), sets a `Badge::Needs(tools)` (pill NEEDS TOOL, see `src/ui/pills.rs`, and the side panel) per affected program (for a single command, its target even if no tool is named), logs "missing tools: …", and, if the run is still `Running`, calls `set_run_phase(RunPhase::Deps { missing, command: captured.deps })`. `command` is the merged `deps:` line; `None` on an unknown distro. `modals.rs:deps_block` shows an intro ("X can't be built yet: cargo is missing." via `missing_sentence`), then either "Run this in a terminal, then press Install again." with the command in a selectable box and `[Copy command]` ("Copied" feedback via `copy_btn`) + `[Close]`, or, without a command, the raw `missing:` lines, "Install these with your package manager…", "Not sure how? Send this to whoever set up your PC." and `[Close] [Copy]`. Update all shows the same block ("Some programs can't be built yet…", "then press Update all again") under the rows.

### Exit 4: Setup

`setup_needed` badges the program(s) (`Badge::SetupNeeded`: the pill SETUP NEEDED and the side panel card "Setup needed" with the copyable `ritz-get install NAME`, `src/ui/side.rs`) and logs "NAME: run `ritz-get install NAME` in a terminal". The single-run dialog (`modals.rs:run_one`, `RunPhase::Setup`, around line 776) says the program needs the user's password for a one-time setup step and that no password prompt could be shown, with the command and `[Copy command] [Close]`. Update all says "Some programs need setup in a terminal. The log has the command for each."

Since commit 6d90aa7 install, remove, switch, reset and update (also Update all) run with `RITZ_GET_SUDO=pkexec` (`Command::uses_sudo`, `src/model.rs`; set in `src/runner.rs`; `delete-data` never gets it), so ritz-get can raise a graphical password prompt itself. Exit 4 therefore only reaches the user when no such prompt could be shown. Treat the Setup dialog as the fallback, not the normal path.

### Exit 5: busy

`ExitKind::Busy`: `finish_progress` clears the bars (nothing happened), the dialog says "Another ritz-get is running. Wait for it to finish, then try again.", and `mutation_finished` returns **before** queueing the refresh list. It logs `busy: another ritz-get is running` only if ritz-get's own stderr line ("another ritz-get is running") was not already the last log line, so exactly one busy line appears.

### "ritz-get not found" banner

`ExitKind::SpawnFailed` in `App::finished`: `state.ritz_get_missing = true`, the run is dropped (no dialog), the rest of the queue is cleared, and "ritz-get not found: install it, then press Retry" is logged. `ui::draw` calls `modals.rs:draw_banner` while `ritz_get_missing`: a red strip with a link to the ritz-get repository and `[Retry]` (`Action::Retry` → `App::begin_startup`, which clears the flag). `AppState::actions_enabled` is false meanwhile (buttons disabled), and the status bar shows "ritz-get not found" in red instead of counts.

### Last-run failures

At startup and after every `update`, `App::reload_last_run` reads ritz-get's `<cache>/ritz-get/last-run` (`last_run_file_from`) with `parse_last_run`: first line `<ISO time> N updated, N skipped, N failed`, optional `failed: NAME NAME` line; anything else is `None`. Effects: the status bar shows "last update failed (see log)" (red, opens `Modal::Log`) when `failed > 0` (`last_update_failed`), and each program named in `failed:` gets the FAILED pill on its card (`AppState::last_failed`, `src/ui/pills.rs`). *Why:* ritz-get's `cmd_update` overwrites that file at the end of every `update` run (no matter who started it), so it reflects the most recent update, whether run from the GUI, a terminal or a scheduled job. This is also why the GUI reloads it after every `update`.

### Porcelain refused

`list_read` maps `PorcelainError::UnsupportedVersion(n)` to `state.refused = Some(n)`, drops queued lists and logs "ritz-get speaks porcelain N, this ritz-get-gui only understands 1: update ritz-get-gui". `modals.rs:refused` outranks every other modal (`draw` returns right after it) and is not dismissible (no Esc / backdrop close): title "ritz-get-gui needs an update", one button `[Update ritz-get-gui]` → `Action::Update { name: Some("ritz-get-gui") }`. *Why:* the app has nothing usable to show until fixed. The button's enabled state deliberately ignores `refused` (it checks only `!busy && !ritz_get_missing`), and `App::run_user_command` lets exactly this self-update through even though `actions_enabled` is false while refused. After it finishes, `mutation_finished` runs `begin_startup` (which clears `refused`). `begin_run_at` does not open the progress dialog while `refused` is set, so that update runs in the background.

### Reset confirmation (`Modal::Reset`)

`modals.rs:reset`, 420 px: "Discard local changes to X?", "N changed files will be lost." (count from the entry's `dirty`), the clone folder path (shown with `~`), and a text that ritz-get downloads a fresh copy and rebuilds it, and that the old version stays if the rebuild fails. Buttons `[Open folder] [Cancel] [Discard and reset]`; Cancel has the focus, so Enter cancels. "Discard and reset" pushes `CloseModal` **then** `Action::Reset`. *Why:* "Close first: the reset opens the progress dialog, which the close must not take away." A failed reset keeps MODIFIED (see run-dialog.md).

### Delete data confirmation (`Modal::DeleteData`)

`modals.rs:delete_data`, 420 px: "Delete X's data?", the data folders with sizes in a scrolling box, a warning that all of it is lost for good, and "Type REPO to confirm" where REPO is the raw repository name (last path segment of the entry's `source`, else the folder name), in a text field that has the focus. `[Delete]` stays disabled until the field holds REPO exactly (trimmed), and Enter in the field does the same, so nothing is deleted by one click or one Enter. Delete is also disabled while the program is still installed, with the text "X is still installed and uses this data. Remove it first, then delete its data." and Cancel focused: ritz-get refuses to delete data of an installed program (`src/ui/modals.rs` header; the run dialog's own `failed_note` handles the `uninstall first` refusal). When no data is left, it says "There is no data left to delete." with `[Close]`. Confirming sends `CloseModal` then `Action::DeleteData`, which opens the progress dialog (verb `DeleteData`, titles "Deleting X's data…" / "Deleted X's data"). The typed text lives in egui temp memory (`delete_data_typed`) and is dropped as soon as another modal shows. The "Delete data…" button in the Remove-done dialog and the side panel's "Delete data…" item only open this modal.

## Using it

Everything here is reactive: read the dialog's text, copy the shown command into a terminal, press the same action again. For a missing ritz-get, install it and press Retry.

## Related links

- [run-dialog.md](run-dialog.md), [log.md](log.md)
- [command-runner](../ritz-get-bridge/command-runner.md) (exit-code mapping, env), [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md)
- [program-actions](../programs/program-actions.md)
- [install-and-desktop](../packaging/install-and-desktop.md)
