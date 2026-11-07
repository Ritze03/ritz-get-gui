# ritz-get-gui — Documentation

ritz-get-gui is a Rust eframe/egui front end that only drives the `ritz-get` CLI. Start with
[architecture/overview.md](architecture/overview.md).

## architecture/ — how the code fits together

- [Overview](architecture/overview.md) — big picture, process model, data flow, module map, where to look for X. **Start here.**
- [vsync off on Wayland](architecture/vsync-off-wayland-design.md) — dated design spec: why the window runs with vsync off.

## features/ — what the app does

ritz-get-bridge:

- [Command runner](features/ritz-get-bridge/command-runner.md) — one `ritz-get` child at a time, env, exit codes, streamed events.
- [Porcelain parsing](features/ritz-get-bridge/porcelain-parsing.md) — `list --porcelain` parser and unsupported-version refusal.

programs:

- [Program list](features/programs/program-list.md) — sections, cards, top bar, side panel, filter, search, sort.
- [Program actions](features/programs/program-actions.md) — install, update, update all, remove, hold, switch, reset, repair, delete data, shortcuts.
- [Icons](features/programs/icons.md) — icon sources, fetching, disk cache, letter fallback.

progress-dialog:

- [Run dialog](features/progress-dialog/run-dialog.md) — progress dialog, per-card bars, run in background.
- [Log](features/progress-dialog/log.md) — log view, status bar Log link, Copy log, full log.
- [Error flows](features/progress-dialog/error-flows.md) — exits 1/3/4/5, missing tools, setup needed, banner, refusal, restart bar.

settings:

- [Settings](features/settings/settings.md) — settings page, persisted keys, folders, touch mode.

packaging:

- [Install and desktop](features/packaging/install-and-desktop.md) — `install.sh`, desktop entry, install layout.
- [Testing and screenshots](features/packaging/testing-and-screenshots.md) — unit and flow tests, fake CLI, shot hook and rig.

## ui/

- [Styling guide](ui/STYLING-GUIDE.md) — Graphite tokens, fonts, panes, buttons, pills, scrolling, with do-NOT bans.

## claude-instructions/ — mandatory rules for agents

- [Working with the Docs](claude-instructions/documentation.md) — read before you touch, update after you change, record the why.
- [Documentation version policy](claude-instructions/documentation-version-policy.md) — date-based, no version numbers.

## meta/

- [Terminology](meta/TERMINOLOGY.md) — project vocabulary and the "ask before acting on an undefined term" rule.
