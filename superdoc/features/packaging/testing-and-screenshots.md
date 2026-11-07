# Testing and screenshots — fake CLI, flow tests, render tests, shot rig

The app is tested without a real `ritz-get` or a display: a fake CLI plus fixtures drive the real runner and app logic, a headless egui render checks layout, and a script photographs the real binary in a private compositor.

## How it works

- **Fake CLI: `tests/fake-ritz-get`.** POSIX sh, used via `RITZ_GET=<abs path>`. Its header documents every variable; the ones that matter:
  - `FAKE_RITZ_GET_FIXTURE` / `FAKE_RITZ_GET_OFFLINE_FIXTURE`: porcelain served by `list --porcelain` / `list --offline`.
  - `FAKE_RITZ_GET_SCENARIO`: `ok` (default), `failed` (exit 1), `deps` (exit 3 with `missing:` and `deps:`), `deps-nodeps` (exit 3, no `deps:`), `needs-root` (exit 4), `busy` (exit 5). Queries (list, branches, `dir`/`shortcuts` without a path) always succeed; deps and needs-root only hit install, update, switch, reset.
  - `FAKE_RITZ_GET_DELAY` (seconds between streamed lines, default 0.05), `FAKE_RITZ_GET_UPDATE_ALL_FILE`, `FAKE_RITZ_GET_NO_PROGRESS` (drop `#start` / `#progress` lines, like an older ritz-get), `FAKE_RITZ_GET_DIR` (what `dir` prints), `FAKE_RITZ_GET_REMOVED_FIXTURE`, `FAKE_RITZ_GET_INSTALLED_FIXTURE`, `FAKE_RITZ_GET_NO_DATA_FLAG` (old ritz-get without `--data`).
  - `FAKE_RITZ_GET_LOG`: one line per call, `start= end= exit= gui= sudo= tty= argv=`. Tests use it to assert exact commands, `RITZ_GET_GUI=1`, `RITZ_GET_SUDO=pkexec` and closed stdin.
  Parsing of its output: [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md).
- **Fixtures: `tests/fixtures/*.porcelain`.** `full` (the main one), `offline` (`#online 0`), `hidden`, `unlisted` (an unlisted clone), `legacy-16`, `bad-version` (`#porcelain 2`, must be refused). `src/porcelain.rs` tests parse them all; `src/sample.rs` (test-only) holds an in-code sample listing of similar shape. Keep fixtures in the exact porcelain format.
- **Flow tests: `src/flow_tests.rs`.** Run the real `App` logic and the real `Runner` against the fake, headless: the test plays `App::update`'s part (poll, handle event, pump, `dispatch`) until idle, then reads the call log. Each `Rig` writes a wrapper script that exports the `FAKE_RITZ_GET_*` variables and hands it to the runner through `App::bin`, *so parallel tests never share the process environment* (stated in the file header). Covers startup call order, every action's argv, the UI per exit code, background runs, queueing behind queries, progress bars, the porcelain-2 refusal. Folder changes from Settings are in `every_action_makes_the_right_call`.
- **Render tests: `src/ui/test_render.rs`.** Run the whole app frame in a real `egui::Context` with the app's fonts and theme and assert nothing paints outside its pane. Sizes (`SIZES`): 1280x800 (side beside, 3 card columns), 900x700 (side beside, 2 columns) and 600x800 (the one narrow size: side stacked, top bar in its narrow form). The whole-app test also runs at `SCALES` 1.0, 1.5, 2.0 (`pixels_per_point`; the logical size stays W x H). `SCENARIOS` lists the states (main, settings, restart and banner, notfound, modals, ...). Select with `cargo test pane`.
- **`RITZ_GET_GUI_SNAPSHOT_DIR`.** When set, `src/ui/test_render.rs:snapshot` rasterises each frame to `<dir>/<name>.png` (names like `app_settings_1280`, with `@1.5` / `@2` at other scales); unset, it does nothing. A software rasteriser for looking at a layout without a window, not a pixel-comparison test.
- **Shot hook: `RITZ_GET_GUI_SHOT`.** Read in `App::new` into `pending_shot`; `src/app.rs:try_apply_shot` applies it once the first listing is in (or the app is idle), through `AppState::apply_shot_hook(spec)` (`src/app.rs:apply_shot_hook`). A spec is `key` or `key=section/name`, e.g. `default`, `select=`, `menu=`, `settings`, `reset=`, `dataleft=`, `deletedata=`, `setupneeded=`, `deps=` / `deps-none=` / `install-deps=`, `install-running=` / `install-done=` / `install-failed=` / `install-hidden=` (`startmenu=` is `install-done=`), `updateall-*`, `progress`, `progress-single=`, `busy`, `restart`, `refused`, `needs`, `empty`, `log`, `logerr`, `remove=` (runs the real `remove` through the fake). It sets initial UI state only, no click automation; unknown specs do nothing. The doc comment on `apply_shot_hook` is the full list. "ritz-get not found", offline and "last update failed" come from the real flow, not from hooks.
- **Screenshot rig: `scripts/shot.sh`.** Runs the real release binary inside a private headless sway with its own runtime dir, Wayland display and sway socket, and desktop variables such as `DISPLAY` unset, so it never touches the live desktop and needs no seat. For each state it launches the app with `RITZ_GET_GUI_SHOT`, sizes the output, waits for the window and grabs it with `grim` into `target/shots/<label>/<state>-<W>.png`, plus `calls-<state>.log` (the fake's call log) and `app-<state>.log`.
  - It points `RITZ_GET` at `tests/fake-ritz-get`, the fixture at `full.porcelain` (`offline.porcelain` for `offline`, `bad-version.porcelain` for `refused`) and `HOME` and the XDG dirs at a throwaway directory. Remote icons are pre-seeded and a dead proxy blocks the network, so shots are offline. Website icons are copied from a sibling ritz-get checkout (`RITZ_GET_REPO`); the script refuses without them unless `--no-web-icons`.
  - Defaults: states from `DEFAULT_STATES`, sizes `1280x800 900x700 600x800` (same as the render tests), plus tall captures for a few states. Options include `--states`, `--widths`, `--scale S` (0.5 to 4; `@S` file suffix), `--fixture`, `--cache-seed`, `--remove-icon ID`, `--relaunch-all`, `--build`, `--allow-stale`, `--check` (prerequisites only). `--help` prints the header.
  - It exits 2 when the binary is older than anything in `src/`, `assets/`, `Cargo.toml` or `Cargo.lock` (a stale build gives placeholder shots); `--build` or `--allow-stale` overrides. Needs `sway`, `swaymsg`, `grim`, `jq`.
  - `--host` is only a label for the output folder (default `local`). Details of the environment a run happens in belong in untracked `.claude/local/`, not here. `LIBGL_ALWAYS_SOFTWARE=1` passes through for machines without a GPU.
  - Screenshots are never committed (`target/` is git-ignored).

## Using it

- Unit, flow and render tests: `cargo test` (`cargo test pane` for layout only).
- Look at layouts without a window: `RITZ_GET_GUI_SNAPSHOT_DIR=<dir> cargo test pane`.
- Real-window shots: `scripts/shot.sh --check`, then `scripts/shot.sh --build --states "settings default"`.
- A new UI state to photograph: add a key to `apply_shot_hook` and list it in `DEFAULT_STATES` in `scripts/shot.sh`. New CLI behaviour to test: add a scenario or variable to `tests/fake-ritz-get` and document it in its header.

## Related links

- [install-and-desktop](install-and-desktop.md)
- [settings](../settings/settings.md)
- [command-runner](../ritz-get-bridge/command-runner.md)
- [porcelain-parsing](../ritz-get-bridge/porcelain-parsing.md)
- [error-flows](../progress-dialog/error-flows.md)
- [Architecture overview](../../architecture/overview.md)
