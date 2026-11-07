# Settings — the Settings view and the GUI's own settings file

The gear tab swaps the program list for a Settings page with three cards: Folders (where `ritz-get` keeps clones and shortcuts), Config (read-only facts) and Input (touch mode). The GUI's own small settings file stores only the sort order and touch mode.

## How it works

- **View.** `src/ui/settings.rs:draw` replaces the groups + side panes while `state.view == View::Settings`; the gear tab toggles it, so the page has no back control. `src/ui/mod.rs:body_settings` wraps it in one `vscroll(state.touch_mode)` area (id `settings_scroll`); `draw` itself has no `ScrollArea`. The column is capped at `MAX_W` = 520 px.
- **Folders card.** Two rows built by `src/ui/settings.rs:folder_row`: "Repo folder" (`Action::SetDir`) and "Shortcut folder" (`Action::SetShortcutsDir`). Each is an editable mono path field plus `Change…`; Enter in the field does the same. `Change…` is disabled unless `state.actions_enabled()`; an empty (trimmed) path pushes nothing. The actions map to `Command::Dir { path: Some(p) }` and `Command::Shortcuts { path: Some(p) }` (`src/model.rs`) and run through the normal runner. `src/flow_tests.rs` (the step list near `Action::SetDir`) shows the calls: `dir PATH --yes`, then `dir` (re-read), then a list refresh; `shortcuts PATH`, then `shortcuts`, then a refresh. See [command-runner](../ritz-get-bridge/command-runner.md).
- **Where the current paths come from.** `state.dir` / `state.shortcuts_dir` are filled at startup by the read-only `dir` and `shortcuts` queries (no argument). They are ritz-get's settings, not the GUI's: nothing about them is written to the GUI settings file. An unknown path shows the hint "unknown".
- **Draft text.** The edit buffer lives in egui memory (`ui.data_mut`), not in `AppState`, and is re-initialised when the state's value changes.
- **Long paths.** Unfocused and too long, the field shows the path with its left elided (`src/ui/settings.rs:elide_left`, snapped to a `/`), full path in the tooltip; focused it shows the real buffer.
- **Layout.** Two columns (a fixed label column; a code comment calls this a deviation from the mockup's 1fr|1fr grid so the path field gets the room), stacked below a 420 px card width (`STACK_BELOW_CARD`).
- **Config card.** "Config file" shows the literal text `~/.config/ritz-get/config` (a static label, not read from anywhere), and "Programs" shows `N installed · M shortcuts` counted from `state.listing` (shortcuts = entries with `ShortcutState::On`).
- **Input card.** One switch (`side::auto_switch`) pushing `Action::SetTouchMode(!state.touch_mode)`.

### The settings file

- **Path.** `src/app.rs:config_file_from` gives `$XDG_CONFIG_HOME/ritz-get-gui/settings`, else `$HOME/.config/ritz-get-gui/settings` (an empty variable counts as unset). Resolved once in `App::new` into `App::config_path`.
- **Format.** `key=value` lines. `src/app.rs:load_sort` reads `sort=<key>` (keys from `src/model.rs:Sort::key`: `default`, `name`, `updates`, `installed`; an unknown value gives the default). `load_touch` is true only for `touch=1`. A missing file or line means defaults (`Sort::Default`, touch off). Loaded once at startup by `App::load_persisted`.
- **Writes.** `App::dispatch` calls `save_sort` / `save_touch` only when `Action::SetSort` / `Action::SetTouchMode` arrive, so the file is created on the first change, never at startup. Both go through `src/app.rs:save_key`, which replaces (or appends) its own line, keeps every other line, creates the directory and rewrites the file. A failed write is logged to the GUI log ("could not save the sort order" / "could not save touch mode"); the in-memory state still changes.
- **Tests.** `src/app.rs:settings_paths_and_sort_round_trip`, `touch_mode_round_trips_and_keeps_other_lines`.

### Touch mode

- Every vertical scroll area is made with `src/ui/mod.rs:vscroll(touch)`, which sets `ScrollSource { drag: touch, ..default }` (`src/ui/mod.rs:scroll_source`). With touch on, dragging the content with a held mouse button scrolls; with it off, drag never scrolls. Mouse wheel, touchpad and scrollbar always work either way.
- Callers: the groups, side, stacked-body and settings areas (`src/ui/mod.rs`), the run dialog's rows (`src/ui/modals.rs`, id `run_rows`) and the log views (`src/ui/logview.rs`, passed `state.touch_mode`). Do NOT build a bare `egui::ScrollArea::vertical()` elsewhere: that area would ignore the setting.
- Test: `src/ui/mod.rs:scroll_config_follows_touch_mode`.

## Using it

Click the gear tab. To move the clones or shortcuts, edit the path and press `Change…` (or Enter); the GUI runs the matching `ritz-get` command and refreshes. The Touch mode switch applies at once and is remembered.

## Options

| Key in `ritz-get-gui/settings` | Default | Meaning |
| --- | --- | --- |
| `sort=` | `default` | Program list order (`default`, `name`, `updates`, `installed`), also set from the top bar Sort control. |
| `touch=1` | off | Touch mode: drag to scroll in every scroll area. |

Cross-checked against the README "Settings" section: same path, same two keys, "off by default", file only written on change, folders are ritz-get's setting.

## Open questions

- Why touch mode is off by default is not stated in the repo, so it is not asserted here.
- The Config card's "Config file" row is a fixed string; whether it is meant to reflect ritz-get's real config path is not stated.

## Related links

- [command-runner](../ritz-get-bridge/command-runner.md)
- [program-actions](../programs/program-actions.md)
- [Architecture overview](../../architecture/overview.md)
