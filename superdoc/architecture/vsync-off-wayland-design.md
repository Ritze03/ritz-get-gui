# vsync off on Wayland — Design Spec

**Date:** 2026-10-30
**Status:** Implemented
**Feature:** The window is created with `vsync: false` so a hidden Wayland surface cannot block the UI thread in `eglSwapBuffers`.

---

## Goal & motivation

On Wayland with `eglSwapInterval 1` (vsync on), `eglSwapBuffers` blocks until the compositor sends a
`wl_surface` frame callback. Compositors such as Hyprland stop sending those to a hidden surface
(another workspace, or covered by a maximised or fullscreen window). The winit event loop then hangs
inside the swap, misses the `xdg_wm_base` ping, and the compositor shows the window as "not
responding".

The goal was to keep the app answering compositor pings while it is not visible. The fix is one
line in `src/main.rs:main`: `eframe::NativeOptions { viewport, vsync: false, ..Default::default() }`
(commit 0720b71, merged by 45f1340, both dated 2026-10-30).

## Non-goals

- No change to the repaint policy. The app already repaints only on demand (see
  [overview](overview.md), "Process model"), so nothing was added to pace frames.
- No Wayland-specific code path: `vsync: false` is set on every platform.

## Key decisions & rationale

- **Turn vsync off instead of working around the blocking swap.** The code comment states the
  cost is nil: "We repaint only on demand (idle = none, animations capped at ~30 fps via
  `request_repaint_after`), so we lose nothing." With no continuous 60 fps render loop, vsync was
  not providing anything the app needed.
- **Where the cap lives:** animations are paced by `ctx.request_repaint_after` in
  `src/app.rs:App::update` (a 33 ms interval while a progress bar runs, plus a timer for finished
  bars to fade), not by the swap interval.
- **Alternatives considered:** the repo records none beyond the above; the commit message and the
  code comment give only this reasoning.
- **Do not turn vsync back on** without re-checking the hidden-surface case: that reintroduces the
  hang.
