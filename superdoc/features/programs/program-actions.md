# Program actions — install, update, remove, switch, reset, hold, open, shortcut, delete data

Everything the user can do to one program or website: the card's primary button, the `...` popover and the side panel's Actions card, all fed by one shared action list, and the path an `Action` takes from a click to a `ritz-get` command.

## How it works

**One action list, three surfaces**

- `src/ui/side.rs:groups` builds the list for an entry; `side.rs:action_list` paints it. The side panel's Actions card and the card `...` popover (`src/ui/groups.rs:popover`) both call `action_list`, so they offer exactly the same rows. `side.rs:offered_actions` (test only) returns the same rows as plain `Action`s for the call-log tests.
- The list is up to four groups separated by hairlines: lifecycle, local, GitHub, and (not installed only) leftover data. Empty groups are dropped.
- Each row is an `Item` with `enabled` and an optional `Action`. `cmd_item` rows (anything that runs `ritz-get`, plus the Reset and Delete data dialogs) are enabled only when `AppState::actions_enabled()`: no mutating command running or queued (a read-only query in flight does not count), ritz-get was found, and the listing is not refused. `free_item` rows (open folder, GitHub links) are always enabled.
- Panes never mutate state: a click pushes `Action`s into a `Vec<Action>`, and `App::dispatch` applies them after the frame.

**What each entry kind offers (`side.rs:groups`)**

| Entry | Rows |
| --- | --- |
| website | Open (`Action::Open`), then Create / Remove shortcut when the shortcut column says so |
| unlisted | Remove only |
| not installed | Install (on the picked branch, via `Entry::install_action`), Create shortcut (disabled), Open folder (disabled), GitHub items, and Delete data… when `D` records exist |
| installed | Update (when `has_update()`), or a disabled "Up to date" row when `status=current`; Repair; Remove; Reset (only when modified); Create / Remove shortcut; Open folder; GitHub items |

- Update on an installed program is `Entry::update_action`: a plain `Update { name }`, or `Switch { name, branch }` when a branch was picked and not yet applied. Install becomes `install NAME --branch B` the same way. *Why:* the in-code docs say a picked branch only changes `pending_branch`; Install / Update apply it, so picking a branch never runs anything by itself (`Action::PickBranch`, `AppState::apply_ui_action`, `AppState::sync_pending`, which also drops a pick that has come true).
- Update is disabled for a modified clone (`u.enabled &= !e.is_modified()`), because update refuses a modified clone. The card button shows the same state with the hint "Local changes: updates are skipped until you reset". Update all skips held and modified programs (`Entry::updatable`).
- Repair is `Action::Repair`, which maps to `install NAME` with no branch (re-runs install.sh); `AppState::verb_of` labels an `Install` of an already installed program as `Verb::Repair`.
- Hold / unhold: the Auto-update switch (`side.rs:auto_switch`) in the side panel's Program card and in the popover. On means "not held". It is enabled only for installed programs. A click sends `Action::Unhold` when `held`, else `Action::Hold`. Caption text comes from `auto_update_caption` (not installed, modified, held, "included in Update all").
- Shortcut rows depend on `ShortcutState` (`src/model.rs`, column 17 of the `E` record): `On` shows Remove shortcut (`shortcut NAME --remove`), `Off` shows Create shortcut (`shortcut NAME`), `None` shows no row at all. Column 17 is `-` when the program has no `NAME.desktop` (and `None` also covers a 16-column line from an older ritz-get), so such a program gets no shortcut action. *Why:* the `ShortcutState` doc says the action is shown only for On/Off.
- Branch picker: the side panel's Program card has a `ComboBox` (`side.rs:branch_combo`); the popover has an expandable Branch row (`groups.rs:branch_item`). Both ask for the list once with `Action::Branches` (`branches NAME`, stdout captured as data) and mark the checked-out branch. Choices are disabled unless `actions_enabled()`.
- Open folder uses `ritz-get dir` output (`state.dir`) joined with the program name and pushes `Action::OpenFolder` (an `xdg-open`, not a ritz-get command). GitHub items (`github_items`): Open on GitHub, Open README (`<repo>/tree/<branch>#readme`), Report issue. They exist only when `Entry::repo_url()` is `Some`.

**The Unlisted card**

An unlisted entry is a clone that is not in the manifest. It is offered Remove and nothing else: its card has no `...` button (`src/ui/groups.rs:actions_row`, `has_more = !e.is_unlisted()`, line 591), its primary button is a danger Remove (`groups.rs:primary`), the action list is `vec![vec![remove()]]` (`src/ui/side.rs:groups`, line 608), and the side panel hides the Program card (branch and auto-update) for it. It also gets no `↗` button on the card. The rest of the interface treats it like an installed program (`is_installed()` is true for `unlisted`), including its status pills.

**The primary button (`groups.rs:primary`)**

| Entry | Button |
| --- | --- |
| website | Open ↗ (`Action::Open`) |
| unlisted | Remove (danger) |
| installed, update available, modified | Update, disabled, with the local-changes hint |
| installed, update available | Update |
| installed, `status=current` | "Up to date", disabled |
| installed, anything else | "Installed", disabled |
| not installed, `state=offline` | Install, disabled, hint "You're offline" |
| not installed | Install |

Every command button is forced to the disabled look unless `actions_enabled()`. While a run owns a card (`state.progress` has its name) the button is replaced by a progress bar (see [../progress-dialog/run-dialog.md](../progress-dialog/run-dialog.md)); a click on a primary button also selects the card first.

**The modified notice (`side.rs:modified_notice`)**

A warn box above the cards for `status=modified`: "You changed files in this program, so updates are paused", then `N changed files · N local commits` from `Entry::dirty` and `Entry::ahead` (counts only; porcelain gives no file names). The way out is the Reset row, which opens `Modal::Reset`, not the reset command. The dialog (`src/ui/modals.rs:reset`) offers Open folder, Cancel (default focus) and Discard and reset; the last pushes `CloseModal` then `Action::Reset`. A similar box for `status=branch-gone` (`branch_gone_notice`) points at the Branch picker.

**Delete data**

Delete data… appears only for a program that is not installed and still has declared data folders. It opens `Modal::DeleteData`, which the user must confirm by typing the repository name (the part of `source` after the last `/`, else the program name); the Delete button is armed only when `actions_enabled()`, the program is not installed, there is data, and the text matches (`modals.rs:delete_data`). It then runs `delete-data NAME --yes`, which never uses sudo (`Command::uses_sudo`). *Why:* the dialog text says the data "will be permanently lost" and "can't be undone"; the typed confirmation is the guard the code applies. An installed program's dialog says to remove it first.

**From click to command**

1. A pane pushes an `Action` (`src/model.rs:Action`).
2. `App::dispatch` (`src/app.rs`): saves sort or touch-mode if that is the action, calls `AppState::apply_ui_action`, which consumes UI-only actions (select, filter, search, sort, modals, branch pick, settings page) and returns the rest.
3. The remainder is handled in `dispatch`: `OpenUrl` / `OpenFolder` go to `xdg-open` detached (`open_detached`), `CopyToClipboard`, `CopyLog`, `OpenRunLog`, `Restart`, `Retry`, `StopRun`; `Update { name: None }` with any updatable program that has a picked branch runs one `switch` per such program, then a plain `update --yes` for the rest; `CheckForUpdates` as described in [program-list.md](program-list.md); everything else goes through `Action::to_command`.
4. `Action::to_command` (`src/model.rs`) is the one mapping to `Command`: Install, Repair (`install NAME`), Update, Remove, Switch, Reset, DeleteData, Hold, Unhold, Open, Shortcut, SetDir, SetShortcutsDir, CheckForUpdates (`list --porcelain --data`), Branches. Actions that are not ritz-get commands return `None`. `Command::args` builds the argv; `Command::uses_sudo` marks install, remove, switch, reset and update for `RITZ_GET_SUDO=pkexec`. Details: [../ritz-get-bridge/command-runner.md](../ritz-get-bridge/command-runner.md).
5. `App::run_user_command` queues the command. Queries may queue while busy. Anything else is dropped (with a "not started (busy or unavailable)" log note) unless `actions_enabled()`.
6. Install, update, switch, reset, remove and delete-data open the progress dialog (`app.rs:opens_run`); hold, unhold, open and shortcut run silently. After a finished mutating run the listing is re-read (`App::mutation_finished` enqueues `list`; `open` is the exception).

**The porcelain-refused exception**

While the listing is refused (the `#porcelain` version is newer than the app knows), `actions_enabled()` is false, so nothing mutating would run. The refused dialog's "Update ritz-get-gui" button is the one fix, so it must bypass that gate in two places:

- The button's enabled check in `src/ui/modals.rs:refused` is `!state.busy && !state.ritz_get_missing`. It deliberately omits `refused`.
- `App::run_user_command` (`src/app.rs`, around line 2063) lets `Update { name: Some("ritz-get-gui") }` through when `refused` is set and nothing is running (`refused_fix`).

*Why:* `modals.rs` comments that `actions_enabled()` "is false while refused, and this button is the fix". When that update finishes, startup runs again and clears `refused` (`App::mutation_finished`, `refused_fix`). Do not "tidy" the check to `actions_enabled()`: the app could then never repair itself.

## Using it

- Click a card, then use the side panel's Actions card; or press the card's primary button for the main action; or open `...` for the full list plus Branch and Auto-update.
- Install / Update / Repair / Remove / Switch / Reset / Delete data show a progress dialog; Hold, Open and shortcut changes just happen and update the card.
- To keep a program out of Update all, turn its Auto-update switch off (hold). To change branch, pick one, then press Update (installed) or Install (not installed).
- To get rid of a modified program's local edits, Reset (discard local changes) opens a confirmation first.

## Related links

- [program-list.md](program-list.md), [icons.md](icons.md)
- [../ritz-get-bridge/command-runner.md](../ritz-get-bridge/command-runner.md), [../ritz-get-bridge/porcelain-parsing.md](../ritz-get-bridge/porcelain-parsing.md)
- [../progress-dialog/run-dialog.md](../progress-dialog/run-dialog.md), [../progress-dialog/error-flows.md](../progress-dialog/error-flows.md)
- [../../architecture/overview.md](../../architecture/overview.md)
