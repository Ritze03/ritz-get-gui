# Install and desktop entry — how ritz-get-gui gets onto a machine

`install.sh` builds the release binary and links it into `~/.local/bin`; ritz-get runs it for install, update and repair. The start-menu entry is a separate file that ritz-get installs, not the script.

## How it works

- **Two sections in `install.sh`.** The part between `# ---- per-program section (edit)` and `# ---- end per-program section` is the only part this project edits: `NAME=ritz-get-gui`, `TARGET=build/$NAME`, `ROOT_LINK=no`, `check_tools`, `build`, `install_files`, `remove_files`. Everything below `# ---- ritz-get plumbing: do not edit below` is ritz-get's template (the header says: copy it to the repo root, edit only the per-program section, SPEC.md wins on disagreement). Do NOT edit the plumbing.
- **Build.** `build()` runs `cargo build --release --locked --color never`, copies `target/release/ritz-get-gui` to `build/.ritz-get-gui.new`, then `mv -f` onto `build/ritz-get-gui`. *Why:* the header calls install idempotent and "safe while running"; a rename over the final name swaps the binary in one step, where overwriting in place would hit a running program. If rustup has no default toolchain, `build` installs `stable` (minimal) and exports `RUSTUP_TOOLCHAIN=stable` without changing the user's default.
- **Tools.** `check_tools` needs `cargo` (`pacman:rust dnf:cargo apt:cargo`). A missing tool prints `missing: TOOL (try: ...)` and the script exits **3**.
- **`ROOT_LINK=no`.** No `/usr/local/bin` link, so no root is needed and the exit-**4** path (`needs root: ...`, plumbing `root_check` / `run_root`) is never taken by this program. `install_files` only does `link "$dir/$TARGET" "$bin/$NAME"`; `remove_files` calls `unlink_ours` (deletes the link only if it points into this clone). Remove never touches config or user data.
- **Other exit codes** (plumbing): 2 usage error; 1 run as root or a failed step. How the GUI shows exit 1/3/4/5 is in [error-flows](../progress-dialog/error-flows.md).
- **Desktop file.** `ritz-get-gui.desktop` (repo root: `Name=Ritz-Get-GUI`, `Exec=ritz-get-gui`, `Icon=ritz-get-gui`, `StartupWMClass=ritz-get-gui`) is installed by ritz-get itself (`ritz-get shortcut ritz-get-gui`), not by `install.sh`; the comment at `install.sh:46` says so. The GUI's "Add it to the start menu?" card runs `ritz-get shortcut NAME` for whichever program was just installed, not specifically this one ([run-dialog](../progress-dialog/run-dialog.md)).
- **Window icon and app_id.** `src/main.rs` embeds `assets/icons/icon-ritz-get-gui-128.png` (`WINDOW_ICON`, `include_bytes!`) as the window icon, sets title `Ritz-Get-GUI` and `with_app_id("ritz-get-gui")`. A code comment says the app_id (Wayland app_id / X11 class) matches the `.desktop` file; `StartupWMClass` repeats it. Keep the three in step, otherwise the shell cannot tie the window to the menu entry. `scripts/shot.sh` also finds the window by `app_id == "ritz-get-gui"`.
- **Icon files.** `icon.svg` is the master and `icon.png` the 1024 px export at the repo root; `assets/icons/` holds the 128 px PNG and an SVG. They are copied artwork, never redrawn. How the app's own icon shows in the list: [icons](../programs/icons.md).
- **`.gitignore`.** Ignores `target/` and `build/` (build output; a comment notes `target/shots/` lives there on purpose so screenshots are never committed), `.claude/` (orchestration state) and `.playwright-mcp/` (tool scratch).
- **vsync.** `src/main.rs` runs with `vsync: false`; the reasoning is in [vsync-off-wayland-design](../../architecture/vsync-off-wayland-design.md).

## Using it

- Via ritz-get: `ritz-get install ritz-get-gui`.
- By hand from a clone: `./install.sh --install --yes`; remove with `./install.sh --remove --yes`. `~/.local/bin` must be on `PATH` (the script prints a note otherwise).

## Related links

- [testing-and-screenshots](testing-and-screenshots.md)
- [settings](../settings/settings.md)
- [Architecture overview](../../architecture/overview.md)
