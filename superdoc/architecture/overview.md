# Architecture Overview

The first doc to read. It maps the code and the data flow, and links out to the feature pages
instead of repeating them. Vocabulary is in [Terminology](../meta/TERMINOLOGY.md); the table of
contents is [README](../README.md).

## Big picture

ritz-get-gui is a Rust eframe/egui (egui 0.33) desktop front end that only ever drives the
`ritz-get` CLI: it runs `ritz-get` commands, parses their output, and shows the result. Everything
hangs off one struct, `src/app.rs:App`, which owns the single mutable `AppState`, the command
runner, the queue, and the icon fetcher. The UI is a pure function of `&AppState`.

Comments in `src/` cite plan labels such as D-numbers and I-numbers; they refer to an external plan
that is not in this repo (see Terminology). Read the code, not the labels.

## Process model

- **One UI thread** runs `App::update` every frame. All state lives there.
- **One `ritz-get` child at a time**, run on a worker thread by `src/runner.rs:Runner`. It streams
  `RunnerEvent`s back over a channel and calls `request_repaint` for every line.
- **A separate icon fetch thread** (`src/icons.rs:IconFetcher`, `ureq` plus a disk cache).
- **Repaint on demand.** `src/main.rs:main` turns vsync off (see
  [vsync-off-wayland-design.md](vsync-off-wayland-design.md)); `App::update` only schedules a
  repaint (about 30 fps) while a progress bar animates or a finished bar is due to fade. Idle
  means no repaints.

*Why:* one command at a time, with the waiting commands in a FIFO queue inside `App` (not in the
runner): the runner runs exactly one command, so `App` starts the next queued one only when
`RunnerEvent::Finished` arrives (`src/app.rs:App::next_to_start`, `App::enqueue`). Identical
queued read-only queries are not duplicated. The code does not state a deeper reason (for example
about ritz-get's own lock), so none is claimed here.

## Data flow

```
main.rs:main            builds the window (1280x800, min 520x420), vsync off
      │  eframe::run_native
      ▼
app.rs:App::new         fonts + theme, load settings/last-run, begin_startup
      │
      ▼
app.rs:App::begin_startup   queues: dir
      │                            list --offline --porcelain --data
      │                            list --porcelain --data
      │                            shortcuts
      ▼
app.rs:App::update  (each frame)
   1. runner.poll()  ──► App::handle_event(RunnerEvent)   [only place runner output touches state]
   2. App::pump      ──► next_to_start ─► Runner::start   [one command at a time]
   3. ui::draw(ctx, &AppState, &mut Vec<Action>)          [read-only]
   4. for each Action: App::dispatch
        ├─ UI-only            ─► AppState::apply_ui_action
        ├─ ritz-get action    ─► Action::to_command ─► run_user_command ─► enqueue
        └─ xdg-open / clipboard / restart / stop ─► handled in-process, not via the runner
   5. App::pump again, schedule repaints
```

In words: at startup `App` queues four commands and the runner executes them in order. Every
output line becomes a `RunnerEvent::Line`; a finished command becomes `RunnerEvent::Finished`
carrying the exit code and a `Captured` summary (stdout, `missing:`, `deps:`, `needs root:`,
per-program result lines). `App::handle_event` and the helpers it calls (`finished`,
`list_finished`, `mutation_finished`, `deps_needed`, `setup_needed`) fold that into `AppState`: a
parsed `Listing`, the log, progress bars, session badges, the Run dialog phase. Then `ui::draw`
paints from `&AppState`, and what the user clicked comes back as `Action`s.

- **Offline list first.** The second startup command is `list --offline --porcelain`; the online
  `list --porcelain` right after replaces it and sets `checked_at`.
  *Why:* the offline listing is the fast first paint (`src/app.rs` module docs: "fast first
  paint"); `dir` goes first because it is tiny and local and the icons need the clone folder early.
- **Panes cannot mutate state.** Every pane is `draw(ui, &AppState, &mut Vec<Action>)` and may only
  push `Action`s (`src/ui/mod.rs` module docs; `src/app.rs` module docs).
  *Why:* `src/model.rs:Action` says actions are applied after drawing, "so a pane never mutates
  state mid-frame"; and `App::handle_event` being the only writer of runner output keeps it
  unit-testable without spawning anything (`src/app.rs` module docs).
- **Gating.** `AppState::actions_enabled` is false while a mutating command runs or waits, when
  `ritz-get` could not be spawned, or when the listing's `#porcelain` version is refused.
  `AppState::busy_queries_only` lets a click queue behind a read-only query. See
  [error-flows](../features/progress-dialog/error-flows.md).
- **`--data`.** `list` is asked for `D` records (`Command::List { data: true }`) unless an old
  ritz-get answered exit 2, after which `App::no_data_flag` drops it for the session.

Details of each stage live on the feature pages:

- Runner, argv, env, exit codes, markers: [command-runner](../features/ritz-get-bridge/command-runner.md)
- Listing parser and refusal: [porcelain-parsing](../features/ritz-get-bridge/porcelain-parsing.md)
- Cards, top bar, side panel, sort/filter: [program-list](../features/programs/program-list.md)
- Install, update, remove, hold, switch, reset, shortcuts: [program-actions](../features/programs/program-actions.md)
- Icon sources and fetching: [icons](../features/programs/icons.md)
- Progress dialog and per-card bars: [run-dialog](../features/progress-dialog/run-dialog.md)
- Log view and Copy log: [log](../features/progress-dialog/log.md)
- Exit 1/3/4/5, banner, refusal, stop: [error-flows](../features/progress-dialog/error-flows.md)
- Settings page, touch mode, folders: [settings](../features/settings/settings.md)
- `install.sh`, `.desktop`, release layout: [install-and-desktop](../features/packaging/install-and-desktop.md)
- Tests, fake CLI, screenshot rig: [testing-and-screenshots](../features/packaging/testing-and-screenshots.md)
- Colours, buttons, scroll rules: [STYLING-GUIDE](../ui/STYLING-GUIDE.md)

## Module map

| Path | Role |
| --- | --- |
| `src/main.rs` | `main`: window, icon, `NativeOptions` (vsync off), `eframe::run_native`. Declares the modules. |
| `src/app.rs` | `AppState` (all UI-visible state), `App` (eframe app: queue, dispatch, events, persistence, screenshot hook) and `App::update`. The hub; also holds most unit tests. |
| `src/model.rs` | Shared types: `Listing`/`Section`/`Entry`, `State`/`Status`/`ShortcutState`, `Command` (argv), `ExitKind`, `Action`, `Modal`, `Badge`, `Filter`/`Sort`, `LogLine`, `LastRun`. |
| `src/porcelain.rs` | Parser for `ritz-get list --porcelain`. |
| `src/runner.rs` | `Runner`: one child on a worker thread, `RunnerEvent`, `Captured`, `#start`/`#progress` marker parsing. |
| `src/icons.rs` | Icon sources, disk cache, fetch thread, `draw`. |
| `src/theme.rs` | Graphite colour tokens, fonts, pills, buttons, pane helpers, `apply`. |
| `src/ui/mod.rs` | Window frame (`draw`, `draw_panes`), `vscroll`, layout breakpoint. |
| `src/ui/topbar.rs` | Top bar and the restart bar. |
| `src/ui/groups.rs` | Section groups and program cards. |
| `src/ui/side.rs` | Side panel for the selected card. |
| `src/ui/bottom.rs` | Status bar (the old bottom log drawer is gone). |
| `src/ui/settings.rs` | Settings page. |
| `src/ui/modals.rs` | Modals (Run dialog, Reset, Delete data, Log) and the ritz-get-missing banner. |
| `src/ui/logview.rs` | Scrolling log view shared by the Run dialog and the Log dialog. |
| `src/ui/pills.rs` | The one mapping from `(state, entry)` to status pills. |
| `src/ui/test_render.rs`, `src/flow_tests.rs`, `src/sample.rs` | Tests only (`cfg(test)`): layout/overflow rendering, whole-app flows against the fake CLI, sample listing. |
| `tests/fake-ritz-get`, `tests/fixtures/` | Fake CLI and porcelain fixtures. |
| `scripts/shot.sh` | Screenshot rig. |
| `design/ritz-get-gui-graphite.html` | The Graphite design mockup the theme follows. |

The window layout is a vertical stack: top bar, optional restart bar, optional ritz-get-missing
banner, body, status bar. The body is groups beside a 300 px side panel at window width of 760 px
or more, stacked below that, or the Settings page in place of both (`src/ui/mod.rs:draw_panes`).

## Where to look for X

- **A new `ritz-get` command:** add a `Command` variant and its `args` in `src/model.rs:Command`
  (and `is_query`/`uses_sudo` as needed), an `Action` variant plus `Action::to_command`, handle it in
  `src/app.rs:App::dispatch` only if it is special; result handling in `App::finished`.
- **A new button or card control:** the pane in `src/ui/` pushes an `Action`; never mutate state.
  Style it per [STYLING-GUIDE](../ui/STYLING-GUIDE.md).
- **What a runner event does to state:** `src/app.rs:App::handle_event` then `App::finished`.
- **A new pill:** `src/theme.rs:PillKind` and `src/ui/pills.rs:pills`.
- **A UI-only state change** (filter, selection, modal): `src/app.rs:AppState::apply_ui_action`.
- **Startup order:** `src/app.rs:App::begin_startup`.
- **A saved setting:** `src/app.rs:load_sort`/`save_sort`/`load_touch`/`save_touch`; see
  [settings](../features/settings/settings.md).
- **Porcelain field handling:** `src/porcelain.rs`, `src/model.rs:Entry`.
- **A screenshot state:** `src/app.rs:AppState::apply_shot_hook` and `scripts/shot.sh`.
- **A layout/overflow regression test:** `src/ui/test_render.rs`.
