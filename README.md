# ritz-get-gui

<img src="icon.png" width="96" align="right" alt="ritz-get-gui icon">

A graphical front end for [ritz-get](https://github.com/Ritze03/ritz-get). Browse Ritze03's
programs, install, update and remove them, switch branches, add start-menu shortcuts, and
update everything at once with a progress display. It does nothing ritz-get can't do on the
command line; it only runs ritz-get for you and shows what happens.

Written in Rust with [egui](https://github.com/emilk/egui).

## Install

With ritz-get, once ritz-get-gui is in its list:

```sh
ritz-get install ritz-get-gui
```

By hand (this is exactly what ritz-get runs, see `install.sh`):

```sh
git clone https://github.com/Ritze03/ritz-get-gui
cd ritz-get-gui
./install.sh --install --yes
```

This builds a release binary into `build/` and links it as `~/.local/bin/ritz-get-gui`. Make
sure `~/.local/bin` is in your `PATH`. The start-menu entry comes from `ritz-get-gui.desktop`
and is added by ritz-get (`ritz-get shortcut ritz-get-gui`). To remove: `./install.sh --remove --yes`.

## Requirements

- [ritz-get](https://github.com/Ritze03/ritz-get) on your `PATH` (or set `RITZ_GET` to its path).
  The GUI says so in its status bar when it can't find it.
- Rust and `cargo`, to build. If missing, `install.sh` prints the package to install.

## How it talks to ritz-get

The GUI only runs `ritz-get` commands, one at a time, and reads their output. Nothing else
touches your programs. Every command runs with `RITZ_GET_GUI=1` and with stdin closed, and the
program list comes from `ritz-get list --porcelain` (a stable format for frontends, described in
ritz-get's `PORCELAIN.md`). Commands that may need root (install, remove, switch, reset) run
with `RITZ_GET_SUDO=pkexec`, so a graphical password prompt appears when needed. If ritz-get
reports a porcelain version this GUI doesn't know, the GUI refuses to guess and tells you to update.

## Settings

`~/.config/ritz-get-gui/settings` (or under `$XDG_CONFIG_HOME`), `key=value` lines:

- `sort=` the sort order of the program list.
- `touch=1` touch mode: drag to scroll in every scroll area (off by default).

Both can be changed in the app; the file is only written when you change something. Where the
clones and shortcuts live is ritz-get's setting, not this one's (`ritz-get dir`, `ritz-get shortcuts`).

## License

MIT, see [LICENSE](LICENSE).

The bundled fonts have their own licences, next to the font files in `assets/fonts/`:
Geist and Geist Mono are under the SIL Open Font License (`Geist-OFL.txt`, `GeistMono-OFL.txt`);
the Nerd Fonts patched Geist Mono is covered by `NerdFonts-LICENSE.txt`.
