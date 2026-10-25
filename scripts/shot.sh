#!/usr/bin/env bash
# shot.sh — headless-sway screenshot rig for ritz-get-gui (D44, D46, D55, D57).
#
# Runs the real binary inside a PRIVATE headless sway (own XDG_RUNTIME_DIR / WAYLAND_DISPLAY /
# SWAYSOCK, no DISPLAY, no seat or logind needed — works over plain ssh), puts it into each state via
# RITZ_GET_GUI_SHOT (no click automation), sets the output to each size and grabs it with grim.
# Never touches the live desktop. Never uses `virsh screenshot`.
#
# Usage:
#   scripts/shot.sh [--bin PATH] [--host NAME] [--states "s1 s2 ..."] [--widths "WxH ..."]
#                   [--fixture PATH] [--ritz-get PATH] [--cache-seed DIR] [--relaunch-all]
#                   [--out DIR] [--settle SECS] [--tall "state ..."] [--tall-size WxH]
#                   [--build] [--allow-stale] [--remove-icon ID] [--scale S]
#   scripts/shot.sh --check          only verify prerequisites, print install hints
#
# Output: <out>/<state>-<W>.png (state names: '/' and '=' become '-'), <out>/calls-<state>.log.
# With --scale S (S != 1) the files are <state>-<W>@S.png (e.g. default-1280@1.5.png), W x S pixels wide.
# Default out = target/shots/<host>/ ; --host default "local" (VM runs: fedora / arch).
#
# States (default set = all of them): the D57 hook specs `default select=ID menu=ID settings reset=ID
# busy progress progress-single=ID restart refused`, the progress-dialog hook specs `install-running=ID
# install-done=ID` (the 2a "Add it to the start menu?" card) `install-failed=ID install-deps=ID deps-none=ID updateall-running
# updateall-done` (+ `install-hidden=ID updateall-hidden`: after "Run in background"), `log` (the log dialog; `logerr` is the same log without the dialog), the I14 hook specs `needs empty`
# (`deps=ID` = `install-deps=ID`, `startmenu=ID` = `install-done=ID`), the QC hook specs `setupneeded=ID` (side panel "Setup needed" card)
# and `select=Unlisted/old-experiment` (Unlisted card), and three
# states that are produced by the real flow instead of a hook (`progress` = a frozen mid-way Update all: per-card bars Updated / Skipped / Up to date / FAILED / Updating... / Queued):
#   offline    the app runs against tests/fixtures/offline.porcelain (`#online 0`, state offline,
#              status unknown)
#   notfound   RITZ_GET points at a path that does not exist -> the "ritz-get not found" banner
#   lastfail   ~/.cache/ritz-get/last-run (in the sandboxed cache dir) reports failures ->
#              "last update failed (see log)" in the status bar and FAILED pills on the cards
# Options:
#   --ritz-get PATH    run the app with RITZ_GET=PATH instead of the fake CLI (e.g. a bogus path)
#   --cache-seed DIR   copy DIR into the sandboxed XDG_CACHE_HOME before every launch (e.g. a
#                      ritz-get/last-run file); the `lastfail` state seeds its own
#   --relaunch-all     relaunch the app for every width of every state. Default: only for `menu=`,
#                      `select=` and `needs`
#                      states (an open popover closes on resize; the select-scroll is one-shot).
#   --build            run `cargo build --release --locked` in the repo first
#   --allow-stale      shot.sh refuses (exit 2) when the binary is older than any file under src/,
#                      assets/, Cargo.toml or Cargo.lock of the repo; this overrides the guard
#   --tall "s1 s2"     states that are ALSO captured at --tall-size (default 1280x1500) as
#                      <state>-1280x1500.png, so a program's side panel shows its Details card.
#                      Only states that are in --states are used. Default: select=Software/geofinder (path row) and `needs` (missing-tool row);
#                      --tall "" turns it off. The normal width set is unchanged.
#   --remove-icon ID   icon-across-uninstall scenario (ID = an installed entry of the fixture, e.g. Helpers/ts6tray):
#                      the fake CLI's `dir` is a temp clone dir holding ID/icon.png (a copy of
#                      assets/icons/icon-ritz-get-gui-128.png), and `remove` deletes it and makes the next
#                      `list` report ID as not installed. States default to "select=ID remove=ID" (before the
#                      remove / right after it: the app itself runs the real `ritz-get remove`); widths to 1280x800.
#                      The entry's source is rewritten to shot-fixture/none so no real remote icon can be fetched.
#   --scale S          display scale (default 1): the sway output is set to (W*S) x (H*S) physical pixels
#                      with `output scale S`, so the app window stays W x H LOGICAL points and egui runs at
#                      pixels_per_point = S (fractional scales go through wp_fractional_scale). The PNGs are
#                      full physical resolution (grim), named with an `@S` suffix. WIDTHS / --tall-size stay
#                      logical. Try 1.25, 1.5, 2.
# Env: LIBGL_ALWAYS_SOFTWARE=1 passes through (set it in VMs without a GPU); SHOT_TIMEOUT (window
# wait, seconds, default 30); ID_SELECT / ID_MENU / ID_RESET / ID_DEPS ... override the entry ids.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Real website icons are copied (never generated) from a ritz-get checkout's icons/*.png into the sandbox clone dir.
RITZ_GET_REPO="${RITZ_GET_REPO:-$ROOT/../ritz-get}"

# Entry ids (section/name, present in tests/fixtures/full.porcelain and src/sample.rs).
ID_SELECT="${ID_SELECT:-Helpers/ts6tray}"                      # behind + setup steps
ID_MENU="${ID_MENU:-Software/yt-dlp-gui}"                      # held, update available
ID_RESET="${ID_RESET:-Software/geofinder}"                     # modified (changed files)
ID_DEPS="${ID_DEPS:-Software/ForzaTelemetryV3Launcher}"        # not installed, needs cargo (title "Forza Telemetry V3 Launcher")
ID_SELECT_WEB="${ID_SELECT_WEB:-Websites/cs2-multimap}"        # website
ID_MENU_MISSING="${ID_MENU_MISSING:-Software/simple-screensharing}"  # not installed, has setup steps
ID_SETUPNEEDED="${ID_SETUPNEEDED:-Software/geofinder}"
ID_UNLISTED="${ID_UNLISTED:-Unlisted/old-experiment}"
TALL_STATES="select=$ID_RESET needs"; TALL_SIZE=1280x1500
DEFAULT_STATES="default select=$ID_SELECT select=$ID_UNLISTED setupneeded=$ID_SETUPNEEDED select=$ID_RESET select=$ID_DEPS select=$ID_SELECT_WEB menu=$ID_MENU menu=$ID_MENU_MISSING settings reset=$ID_RESET install-running=$ID_MENU_MISSING install-done=$ID_MENU_MISSING install-failed=$ID_MENU_MISSING install-deps=$ID_DEPS deps-none=$ID_DEPS updateall-running updateall-done install-hidden=$ID_MENU_MISSING updateall-hidden log busy progress progress-single=Helpers/ts6tray restart refused needs empty offline notfound lastfail"
DEFAULT_WIDTHS="1280x800 900x700 600x800"

BIN="$ROOT/target/release/ritz-get-gui"; HOST=local; STATES="$DEFAULT_STATES"; WIDTHS="$DEFAULT_WIDTHS"
FIXTURE=""; OUT=""; CHECK=0; SETTLE=1.5; TIMEOUT="${SHOT_TIMEOUT:-30}"
RITZ_GET_OVERRIDE=""; CACHE_SEED=""; RELAUNCH_ALL=0; REMOVE_ICON=""
BUILD=0; ALLOW_STALE=0; SCALE=1; WIDTHS_SET=0; STATES_SET=0

die() { echo "shot.sh: $*" >&2; exit 2; }
usage() { awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "${BASH_SOURCE[0]}"; }
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="${2:?}"; shift 2 ;;
    --host) HOST="${2:?}"; shift 2 ;;
    --states) STATES="${2:?}"; STATES_SET=1; shift 2 ;;
    --widths) WIDTHS="${2:?}"; WIDTHS_SET=1; shift 2 ;;
    --fixture) FIXTURE="${2:?}"; shift 2 ;;
    --ritz-get) RITZ_GET_OVERRIDE="${2:?}"; shift 2 ;;
    --cache-seed) CACHE_SEED="${2:?}"; shift 2 ;;
    --remove-icon) REMOVE_ICON="${2:?}"; shift 2 ;;
    --relaunch-all) RELAUNCH_ALL=1; shift ;;
    --out) OUT="${2:?}"; shift 2 ;;
    --settle) SETTLE="${2:?}"; shift 2 ;;
    --tall) TALL_STATES="${2-}"; shift 2 ;;
    --tall-size) TALL_SIZE="${2:?}"; shift 2 ;;
    --build) BUILD=1; shift ;;
    --allow-stale) ALLOW_STALE=1; shift ;;
    --scale) SCALE="${2:?}"; shift 2 ;;
    --check) CHECK=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1 (try --help)" ;;
  esac
done
[[ "$BIN" = /* ]] || BIN="$PWD/$BIN"
[[ -z "$CACHE_SEED" || -d "$CACHE_SEED" ]] || die "--cache-seed: $CACHE_SEED is not a directory"
if [[ -n "$REMOVE_ICON" ]]; then
  [[ $STATES_SET == 1 ]] || STATES="select=$REMOVE_ICON remove=$REMOVE_ICON"
  [[ $WIDTHS_SET == 1 ]] || WIDTHS=1280x800
  TALL_STATES=""
fi
[[ "$SCALE" =~ ^[0-9]+(\.[0-9]+)?$ ]] && awk -v s="$SCALE" 'BEGIN{exit !(s>=0.5 && s<=4)}' || die "--scale: '$SCALE' is not a number between 0.5 and 4"
# File-name suffix: none at scale 1 (also 1.0 / 1.00), `@S` otherwise.
SUFFIX=""; awk -v s="$SCALE" 'BEGIN{exit !(s==1)}' || SUFFIX="@$SCALE"
[[ -n "$OUT" ]] || OUT="$ROOT/target/shots/$HOST"

# --- build / stale-binary guard ----------------------------------------------------------------
if [[ $BUILD == 1 ]]; then
  (cd "$ROOT" && cargo build --release --locked) || die "cargo build failed"
  ALLOW_STALE=1   # cargo just ran: it decides what is up to date (a touched file with unchanged content keeps the old binary mtime)
fi
if [[ $ALLOW_STALE == 0 && -x "$BIN" ]]; then
  newer="$(find "$ROOT/src" "$ROOT/assets" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" -type f -newer "$BIN" -print -quit 2>/dev/null)"
  [[ -z "$newer" ]] || die "binary $BIN is older than $newer (stale build, placeholder shots?). Rebuild (cargo build --release --locked, or --build) or pass --allow-stale."
fi

# --- prerequisites ---------------------------------------------------------------------------
missing=0
need() {  # tool pacman-pkg dnf-pkg
  command -v "$1" >/dev/null 2>&1 && return
  echo "missing: $1   (pacman -S $2 | dnf install $3)" >&2; missing=1
}
need sway sway sway
need swaymsg sway sway
need grim grim grim
need jq jq jq
if [[ ! -x "$BIN" ]]; then
  echo "missing: binary $BIN   (build it: cargo build --release --locked)" >&2; missing=1
fi
if [[ $CHECK == 1 ]]; then
  [[ $missing == 0 ]] && echo "shot.sh: all prerequisites present"
  exit $missing
fi
[[ $missing == 0 ]] || exit 2

# --- fake CLI / fixtures (graceful if absent) ----------------------------------------------------
FAKE="$ROOT/tests/fake-ritz-get"
if [[ -x "$FAKE" ]]; then USE_FAKE=1; else USE_FAKE=0; echo "shot.sh: warning: $FAKE not found; running without RITZ_GET (app shows its own data)" >&2; fi
[[ -n "$FIXTURE" ]] || FIXTURE="$ROOT/tests/fixtures/full.porcelain"
BAD_FIXTURE="$ROOT/tests/fixtures/bad-version.porcelain"
OFFLINE_FIXTURE="$ROOT/tests/fixtures/offline.porcelain"
[[ $USE_FAKE == 0 || -f "$FIXTURE" ]] || echo "shot.sh: warning: fixture $FIXTURE not found" >&2

mkdir -p "$OUT" || die "cannot create $OUT"

# --- private sway ------------------------------------------------------------------------------------
RT="$(mktemp -d "${TMPDIR:-/tmp}/ritz-shot-XXXXXX")" || die "mktemp failed"
chmod 700 "$RT"
SWAY_PID=""; APP_PID=""
cleanup() {
  [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null
  if [[ -n "$SWAY_PID" ]]; then
    kill "$SWAY_PID" 2>/dev/null
    for _ in $(seq 20); do kill -0 "$SWAY_PID" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$SWAY_PID" 2>/dev/null
  fi
  rm -rf "$RT"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Scrub anything that could point at the live desktop, then set our own.
unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET SWAYSOCK I3SOCK DBUS_SESSION_BUS_ADDRESS
export XDG_RUNTIME_DIR="$RT" SWAYSOCK="$RT/sway.sock"
export XDG_CACHE_HOME="$RT/cache" XDG_CONFIG_HOME="$RT/config" XDG_DATA_HOME="$RT/data" HOME="$RT/home"
mkdir -p "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$HOME"

cat >"$RT/sway.conf" <<'CONF'
default_border none
default_floating_border none
focus_follows_mouse no
for_window [app_id="ritz-get-gui"] fullscreen enable
CONF

first="${WIDTHS%% *}"; W0="${first%x*}"; H0="${first#*x}"
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c "$RT/sway.conf" >"$OUT/sway.log" 2>&1 &
SWAY_PID=$!

wait_for() {  # secs description cmd...
  local n=$(( $1 * 10 )) desc="$2"; shift 2
  for _ in $(seq "$n"); do
    "$@" >/dev/null 2>&1 && return 0
    kill -0 "$SWAY_PID" 2>/dev/null || break
    sleep 0.1
  done
  echo "shot.sh: timed out waiting for $desc (see $OUT/sway.log)" >&2; return 1
}
wait_for 20 "sway IPC" swaymsg -t get_version || exit 1
WL=""
for _ in $(seq 50); do
  WL="$(cd "$RT" && compgen -G 'wayland-[0-9]*' | grep -v '\.lock$' | head -1)"
  [[ -n "$WL" ]] && break
  sleep 0.1
done
[[ -n "$WL" ]] || { echo "shot.sh: sway made no wayland socket" >&2; exit 1; }
export WAYLAND_DISPLAY="$WL"
OUTPUT="$(swaymsg -t get_outputs | jq -r '.[0].name')"
[[ -n "$OUTPUT" && "$OUTPUT" != null ]] || { echo "shot.sh: sway has no output" >&2; exit 1; }

set_size() {  # W H (logical points): the output gets W*SCALE x H*SCALE physical pixels, so the window stays W x H
  local pw ph
  pw="$(awk -v v="$1" -v s="$SCALE" 'BEGIN{printf "%d", v*s+0.5}')"
  ph="$(awk -v v="$2" -v s="$SCALE" 'BEGIN{printf "%d", v*s+0.5}')"
  swaymsg "output $OUTPUT resolution ${pw}x${ph} scale $SCALE" >/dev/null
}
# App window size as sway sees it ("WxH"), empty if no window yet.
win_size() {
  swaymsg -t get_tree | jq -r '[.. | objects | select(.app_id? == "ritz-get-gui")][0] | select(. != null) | "\(.rect.width)x\(.rect.height)"'
}
wait_size() {  # WxH
  local want="$1" got=""
  for _ in $(seq $(( TIMEOUT * 10 ))); do
    got="$(win_size)"
    [[ "$got" == "$want" ]] && return 0
    kill -0 "$APP_PID" 2>/dev/null || { echo "shot.sh: app exited early" >&2; return 1; }
    sleep 0.1
  done
  echo "shot.sh: window size is '${got:-<no window>}', wanted $want" >&2; return 1
}

# Real art for a program of the fixture, COPIED (never created): the user's local repo (a sibling of
# $RITZ_GET_REPO), the installed clone under ritz-get's src dir, or this repo's own icon.png. Prints the
# path, or nothing (the program then honestly shows its letter).
REAL_HOME="$(getent passwd "$(id -un)" | cut -d: -f6)"
find_art() {  # name owner/repo
  local name="$1" repo="${2##*/}" c
  [[ "$name" == ritz-get-gui ]] && { echo "$ROOT/icon.png"; return; }
  for c in "${RITZ_GET_REPO%/*}/$repo/icon.png" "$REAL_HOME/.local/share/ritz-get/src/$name/icon.png"; do
    [[ -f "$c" ]] && { echo "$c"; return; }
  done
}

# Fake `ritz-get dir`: <dir>/ritz-get/icons/NAME.png (real website icons, copied), the app's own
# <dir>/ritz-get-gui/icon.png, and <dir>/NAME/icon.png for every program of fixture $2 that has real art
# (installed ones show it from there; it also covers hooks that flip an entry to installed, e.g. install-done).
seed_clones() {  # dir fixture
  mkdir -p "$1/ritz-get/icons" "$1/ritz-get-gui"
  cp "$ROOT/icon.png" "$1/ritz-get-gui/icon.png" 2>/dev/null
  cp "$RITZ_GET_REPO"/icons/*.png "$1/ritz-get/icons/" 2>/dev/null || true
  local name src art
  while IFS=$'\t' read -r name src; do
    art="$(find_art "$name" "$src")"
    [[ -n "$art" ]] && { mkdir -p "$1/$name"; cp "$art" "$1/$name/icon.png"; }
  done < <(awk -F'\t' '$1=="E" && $5=="program" && $6 ~ /^[^\/ ]+\/[^\/ ]+$/ {print $3 "\t" $6}' "${2:-$FIXTURE}")
}

# Pre-seed the GUI's icon cache for every not-installed GitHub program of fixture $1 that has real art:
# <cache>/ritz-get-gui/icons/<owner>_<repo>_<branch|HEAD>.png + .sha (the fixture's latest sha), so the app
# never fetches (src/icons.rs: Remote cache_path / sidecar). Entries without art stay letters.
seed_icon_cache() {  # fixture
  local d="$XDG_CACHE_HOME/ritz-get-gui/icons" name src branch latest art owner repo rf sha
  mkdir -p "$d"
  while IFS=$'\t' read -r name src branch latest; do
    art="$(find_art "$name" "$src")"; [[ -n "$art" ]] || continue
    owner="${src%%/*}"; repo="${src##*/}"
    rf="$branch"; [[ -z "$rf" || "$rf" == - ]] && rf=HEAD
    rf="$(printf '%s' "$rf" | sed 's/[^A-Za-z0-9.-]/-/g')"
    sha="${latest##* }"; [[ "$sha" == - ]] && sha=""
    cp "$art" "$d/${owner}_${repo}_${rf}.png"
    printf '%s' "$sha" >"$d/${owner}_${repo}_${rf}.sha"
  done < <(awk -F'\t' '$1=="E" && $5=="program" && $7!="installed" && $6 ~ /^[^\/ ]+\/[^\/ ]+$/ {print $3 "\t" $6 "\t" $10 "\t" $12}' "$1")
}

# Launch the app in `state` (sets APP_PID). The cache dir is reset first, then seeded: --cache-seed,
# and for `lastfail` a ritz-get/last-run naming two programs of the fixture.
launch() {  # state name
  local state="$1" name="$2" fx="$FIXTURE"
  rm -rf "${XDG_CACHE_HOME:?}"/* "${XDG_CONFIG_HOME:?}"/* 2>/dev/null
  [[ -n "$CACHE_SEED" ]] && cp -a "$CACHE_SEED"/. "$XDG_CACHE_HOME"/
  seed_icon_cache "$([[ $state == offline ]] && echo "$OFFLINE_FIXTURE" || echo "$fx")"
  if [[ "$state" == lastfail ]]; then
    mkdir -p "$XDG_CACHE_HOME/ritz-get"
    printf '2026-10-24T03:00:01 1 updated, 1 skipped, 2 failed\nfailed: geofinder ts6tray\n' >"$XDG_CACHE_HOME/ritz-get/last-run"
  fi
  # Shots never touch the network: every remote icon is pre-seeded; a stray fetch dies at a dead proxy.
  local envs=(RITZ_GET_GUI_SHOT="$state" ALL_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 HTTP_PROXY=http://127.0.0.1:9 NO_PROXY=)
  if [[ -n "$REMOVE_ICON" ]]; then
    # fresh clone dir + before/after fixtures for every launch (the fake `remove` deletes the clone)
    local rname="${REMOVE_ICON##*/}" clones="$RT/clones"
    rm -rf "$clones" "$RT"/removed.porcelain*; seed_clones "$clones" "$fx"; mkdir -p "$clones/$rname"
    cp "$ROOT/assets/icons/icon-ritz-get-gui-128.png" "$clones/$rname/icon.png"
    awk -F'\t' -v OFS='\t' -v id="$REMOVE_ICON" '$1=="E" && $2"/"$3==id { $6="shot-fixture/none" } {print}' "$FIXTURE" >"$RT/installed.porcelain"
    awk -F'\t' -v OFS='\t' -v id="$REMOVE_ICON" '$1=="E" && $2"/"$3==id { $6="shot-fixture/none"; $7="available"; $8="-"; $9=0; $11="-"; $13="-"; $14="-"; $15="-"; $17="-" } {print}' "$FIXTURE" >"$RT/removed.porcelain"
    fx="$RT/installed.porcelain"
    envs+=(FAKE_RITZ_GET_DIR="$clones" FAKE_RITZ_GET_REMOVED_FIXTURE="$RT/removed.porcelain")
  fi
  if [[ -z "$REMOVE_ICON" ]]; then
    local clones="$RT/clones"; rm -rf "$clones"; seed_clones "$clones" "$fx"
    envs+=(FAKE_RITZ_GET_DIR="$clones")
  fi
  if [[ $USE_FAKE == 1 ]]; then
    [[ "$state" == refused ]] && fx="$BAD_FIXTURE"
    [[ "$state" == offline ]] && fx="$OFFLINE_FIXTURE"
    : >"$OUT/calls-$name.log"
    envs+=(RITZ_GET="$FAKE" FAKE_RITZ_GET_FIXTURE="$fx" FAKE_RITZ_GET_OFFLINE_FIXTURE="$fx" FAKE_RITZ_GET_LOG="$OUT/calls-$name.log")
  fi
  [[ "$state" == notfound ]] && envs+=(RITZ_GET="$RT/no-such-ritz-get")
  [[ -n "$RITZ_GET_OVERRIDE" ]] && envs+=(RITZ_GET="$RITZ_GET_OVERRIDE")
  env "${envs[@]}" "$BIN" >>"$OUT/app-$name.log" 2>&1 &
  APP_PID=$!
}
stop_app() {
  [[ -n "$APP_PID" ]] || return 0
  kill "$APP_PID" 2>/dev/null; wait "$APP_PID" 2>/dev/null; APP_PID=""
}

# --- main loop ----------------------------------------------------------------------------------------
fails=0; written=(); t0=$SECONDS
run_state() {  # state widths tall(0/1)
  local state="$1" widths="$2" tall="$3" name relaunch wh png
  name="${state//[\/=]/-}"
  : >"$OUT/app-$name.log"
  relaunch=$RELAUNCH_ALL
  # A popover closes on resize, and the scroll-to-side-panel of a selection is a one-frame request
  # that only the stacked (< 760 px) layout acts on: those states start fresh at every width.
  [[ "$state" == menu=* || "$state" == select=* || "$state" == needs || "$state" == setupneeded=* || "$state" == progress-single=* ]] && relaunch=1
  for wh in $widths; do
    png="$OUT/$name-${wh%x*}$SUFFIX.png"
    [[ $tall == 1 ]] && png="$OUT/$name-$wh$SUFFIX.png"
    rm -f "$png"
    set_size "${wh%x*}" "${wh#*x}"
    [[ -n "$APP_PID" ]] || launch "$state" "$name"
    if wait_size "$wh"; then
      sleep "$SETTLE"            # let egui repaint at the new size (software GL is slow)
      if wait_size "$wh" && grim -o "$OUTPUT" "$png" && [[ -s "$png" ]]; then
        written+=("$png")
      else
        echo "shot.sh: FAILED $state @ $wh (grim)" >&2; fails=$((fails+1))
      fi
    else
      echo "shot.sh: FAILED $state @ $wh (log: $OUT/app-$name.log)" >&2; fails=$((fails+1))
      stop_app
    fi
    [[ $relaunch == 1 ]] && stop_app
  done
  stop_app
}
for state in $STATES; do run_state "$state" "$WIDTHS" 0; done
for state in $TALL_STATES; do
  [[ " $STATES " == *" $state "* ]] && run_state "$state" "$TALL_SIZE" 1
done

echo "shot.sh: ${#written[@]} screenshot(s) in $OUT, $fails failure(s), $(( SECONDS - t0 )) s"
printf '  %s\n' "${written[@]}"
[[ $fails == 0 ]]
