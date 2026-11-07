# Terminology

Project-specific vocabulary. When the user uses a term defined here, use the same
meaning.

**If the user uses a non-standard term that is not defined here, ask what they mean by
it before acting on it.** Once its meaning is clear, add it to this file. Keep this
file up to date automatically: whenever the user introduces or clarifies a term, add or
amend the entry here.

**Keep every entry short: what the term is, and where to read more.** One or two
sentences, about 200 characters. Details, behaviour, option names, history and rationale
belong on the linked doc page. If that page doesn't exist and there is more to say, write
the page; don't grow the entry. When you amend an entry, trim it to this form instead of
appending. This file stays in context for the whole session.

## Terms

- **ritz-get** — The CLI this app drives and never bypasses. See `superdoc/features/ritz-get-bridge/command-runner.md`.
- **porcelain** — ritz-get's stable machine-readable output (`list --porcelain`). See `superdoc/features/ritz-get-bridge/porcelain-parsing.md`.
- **Listing / Entry / Section** — Parsed `list` output: a Listing has Sections (manifest order) of Entries (one program or website). See `superdoc/features/programs/program-list.md`.
- **Unlisted** — Section (always last) of installed clones not in the manifest. See `superdoc/features/programs/program-list.md`.
- **Held** — A program with auto-update off; a flag, not a Status. See `superdoc/features/programs/program-actions.md`.
- **Behind / Modified / Ahead / Dirty** — Commits behind upstream; local changes; local commits not upstream; changed-file count. See `superdoc/features/programs/program-list.md`.
- **Status vs State** — State is the entry's kind of presence (installed, available, offline, ...); Status is how an installed one compares to upstream. See `superdoc/features/ritz-get-bridge/porcelain-parsing.md`.
- **offline listing** — `list --offline`: the fast first paint at startup, replaced by the online list. See `superdoc/architecture/overview.md`.
- **Update all (N)** — Top-bar button; N counts programs that are behind, not held, not modified. See `superdoc/features/programs/program-list.md`.
- **Run / Progress dialog / Run in background** — Modal for a running install/update/etc.; "Run in background" closes it while the command continues. See `superdoc/features/progress-dialog/run-dialog.md`.
- **session badge** — NEEDS tool / SETUP NEEDED pill set from exit 3/4 in this session, not from porcelain. See `superdoc/features/progress-dialog/error-flows.md`.
- **exit kinds 0-5** — ritz-get exit codes as `ExitKind`: ok, failed, usage, missing tools, needs root, busy. See `superdoc/features/ritz-get-bridge/command-runner.md`.
- **Missing tools** — Run dialog phase for exit 3, showing the `deps:` install command. See `superdoc/features/progress-dialog/error-flows.md`.
- **Setup needed** — Exit 4: the program needs a root setup step in a terminal. See `superdoc/features/progress-dialog/error-flows.md`.
- **pkexec / RITZ_GET_SUDO** — Env var the runner sets to `pkexec` for root-capable commands (graphical password prompt). See `superdoc/features/ritz-get-bridge/command-runner.md`.
- **Last run** — ritz-get's last-update record (`failed:` names mark cards). See `superdoc/features/progress-dialog/error-flows.md`.
- **Shortcut (on, off, `-`)** — Start-menu entry state of an installed program; `-` = not installed or no `.desktop` file. See `superdoc/features/programs/program-actions.md`.
- **Hold / Unhold** — Turn a program's auto-update off / on. See `superdoc/features/programs/program-actions.md`.
- **Switch** — Change an installed program's branch (`switch NAME BRANCH`). See `superdoc/features/programs/program-actions.md`.
- **Reset** — Discard local changes (`reset NAME --yes`), after a confirm modal. See `superdoc/features/programs/program-actions.md`.
- **Repair** — Re-run install on an installed program (`install NAME`). See `superdoc/features/programs/program-actions.md`.
- **Delete data** — Remove an uninstalled program's data folders (`delete-data`). See `superdoc/features/programs/program-actions.md`.
- **Restart bar** — Strip shown after ritz-get-gui updated itself, offering Restart. See `superdoc/features/programs/program-list.md`.
- **Touch mode** — Setting: drag-to-scroll in every scroll area. See `superdoc/features/settings/settings.md`.
- **Graphite** — The app's single dark theme (colour tokens in `src/theme.rs`). See `superdoc/ui/STYLING-GUIDE.md`.
- **Pill** — Small uppercase status label on a card; one mapping in `src/ui/pills.rs`. See `superdoc/ui/STYLING-GUIDE.md`.
- **Pane / Action** — A pane draws a region from `&AppState`; it pushes Actions, which `App::dispatch` applies. See `superdoc/architecture/overview.md`.
- **Shot hook** — `RITZ_GET_GUI_SHOT` env var that puts the UI in a named state for screenshots. See `superdoc/features/packaging/testing-and-screenshots.md`.
- **Fake CLI** — `tests/fake-ritz-get`, a scripted stand-in for ritz-get used by tests and screenshots. See `superdoc/features/packaging/testing-and-screenshots.md`.
- **D-numbers / I-numbers** — Labels like D54 or I7 in code comments; they reference an external plan that is not in this repo. Do not guess their meaning.
