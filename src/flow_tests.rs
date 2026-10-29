//! Whole-app flow tests (I14): the real [`App`] logic and the real [`Runner`] against
//! `tests/fake-ritz-get`, headless (no window, no `eframe`): the test plays `App::update`'s part
//! (`poll` -> `handle_event` -> `pump`, then `dispatch` for the actions a pane would push) until
//! the app is idle, and then reads the fake CLI's call log (`FAKE_RITZ_GET_LOG`, format in the
//! header of `tests/fake-ritz-get`).
//!
//! Checks (plan "Done when"):
//! * startup runs `dir`, `list --offline --porcelain --data`, THEN `list --porcelain --data`, `shortcuts`
//!   (every list carries `--data`; an old ritz-get that exits 2 on it is asked again without);
//! * every call has `gui=1` and `tty=0`; only install / remove / switch / reset have
//!   `sudo=pkexec` (`delete-data` never does); `update`, `reset` and `dir PATH` always carry `--yes`;
//! * the shortcut action is `shortcut NAME` for column `off`, `shortcut NAME --remove` for `on`,
//!   and the action list offers none for `-`;
//! * no two calls overlap;
//! * each exit code shows its UI in the progress dialog (1 FAILED, 3 Missing tools, 4 SETUP NEEDED, 5 busy);
//! * install / update / Update all open the progress dialog; Run in background hides it and only a
//!   result that needs the user (start-menu question, failure, missing tools) brings it back;
//! * a read-only query in flight (`branches`, the startup lists) does not disable the actions, and
//!   an action clicked meanwhile queues behind it;
//! * `#porcelain 2` is refused and the dialog's "Update ritz-get-gui" works.
//!
//! Each [`Rig`] gets a wrapper script that exports the `FAKE_RITZ_GET_*` variables and execs the
//! fake, and hands that to the runner through [`App::bin`], so parallel tests never share the
//! process environment.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::ui::side;

const FAKE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake-ritz-get");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// One line of the fake CLI's call log.
#[derive(Clone, Debug)]
struct Call {
    start: u128,
    end: u128,
    exit: i32,
    gui: String,
    sudo: String,
    tty: String,
    argv: String,
}

impl Call {
    fn words(&self) -> Vec<&str> {
        self.argv.split_whitespace().collect()
    }
}

/// `1761300000.123456789` -> nanoseconds since the epoch.
fn nanos(t: &str) -> u128 {
    let (s, f) = t.split_once('.').unwrap_or((t, "0"));
    let frac = format!("{f:0<9}");
    s.parse::<u128>().unwrap() * 1_000_000_000 + frac[..9].parse::<u128>().unwrap()
}

fn parse_log(text: &str) -> Vec<Call> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            // start=.. end=.. exit=.. gui=.. sudo=.. tty=.. argv=<rest of the line>
            let (head, argv) = l.split_once(" argv=").unwrap_or_else(|| panic!("bad call-log line: {l}"));
            let field = |k: &str| -> String {
                head.split_whitespace()
                    .find_map(|w| w.strip_prefix(&format!("{k}=")))
                    .unwrap_or_else(|| panic!("no {k}= in call-log line: {l}"))
                    .to_owned()
            };
            Call {
                start: nanos(&field("start")),
                end: nanos(&field("end")),
                exit: field("exit").parse().unwrap(),
                gui: field("gui"),
                sudo: field("sudo"),
                tty: field("tty"),
                argv: argv.to_owned(),
            }
        })
        .collect()
}

/// The invariants that hold for EVERY call the GUI makes (D48, PORCELAIN.md).
fn assert_call_invariants(calls: &[Call]) {
    assert!(!calls.is_empty(), "the call log is empty");
    for c in calls {
        let w = c.words();
        let cmd = w.first().copied().unwrap_or("");
        assert_eq!(c.gui, "1", "RITZ_GET_GUI=1 on every call: {c:?}");
        assert_eq!(c.tty, "0", "stdin is never a terminal: {c:?}");
        let sudo = matches!(cmd, "install" | "remove" | "switch" | "reset");
        assert_eq!(c.sudo, if sudo { "pkexec" } else { "-" }, "RITZ_GET_SUDO only on install/remove/switch/reset: {c:?}");
        if matches!(cmd, "update" | "reset") {
            assert!(w.contains(&"--yes"), "{cmd} always carries --yes: {c:?}");
        }
        if cmd == "dir" && w.len() > 1 {
            assert!(w.contains(&"--yes"), "dir PATH always carries --yes: {c:?}");
        }
        assert!(c.end >= c.start, "{c:?}");
    }
    for pair in calls.windows(2) {
        assert!(
            pair[1].start >= pair[0].end,
            "calls overlap (one command at a time):\n  {:?}\n  {:?}",
            pair[0],
            pair[1]
        );
    }
}

fn argvs(calls: &[Call]) -> Vec<&str> {
    calls.iter().map(|c| c.argv.as_str()).collect()
}

static RIGS: AtomicUsize = AtomicUsize::new(0);

/// An [`App`] wired to the fake CLI in a private temp dir.
struct Rig {
    app: App,
    ctx: egui::Context,
    dir: PathBuf,
    log: PathBuf,
    listing: PathBuf,
}

impl Rig {
    /// `scenario` = `FAKE_RITZ_GET_SCENARIO`, `fixture` = a file in `tests/fixtures/`.
    fn new(scenario: &str, fixture: &str) -> Rig {
        Rig::new_with(scenario, fixture, None)
    }

    /// `installed` = `(entry name, column 17)`: after a successful `install`, the listing is `fixture`
    /// with that entry rewritten to installed/current and that shortcut column (D47).
    fn new_with(scenario: &str, fixture: &str, installed: Option<(&str, &str)>) -> Rig {
        Rig::new_env(scenario, fixture, installed, "")
    }

    /// [`Rig::new_with`] plus `more`: `NAME=value ...` words exported to the fake after the defaults
    /// (e.g. a `FAKE_RITZ_GET_DELAY` slow enough to watch the state change between streamed lines);
    /// `{DIR}` is the rig's temp dir.
    fn new_env(scenario: &str, fixture: &str, installed: Option<(&str, &str)>, more: &str) -> Rig {
        let n = RIGS.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("rgg-flow-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listing = dir.join("listing.porcelain");
        std::fs::copy(format!("{FIXTURES}/{fixture}"), &listing).unwrap();
        let log = dir.join("calls.log");
        let mut extra = String::new();
        if let Some((name, col17)) = installed {
            let text: String = std::fs::read_to_string(&listing)
                .unwrap()
                .lines()
                .map(|l| {
                    let mut f: Vec<&str> = l.split('\t').collect();
                    if f.len() >= 17 && f[0] == "E" && f[2] == name {
                        f[6] = "installed";
                        f[7] = "current";
                        f[16] = col17;
                    }
                    f.join("\t") + "\n"
                })
                .collect();
            let after = dir.join("installed.porcelain");
            std::fs::write(&after, text).unwrap();
            extra = format!("FAKE_RITZ_GET_INSTALLED_FIXTURE='{}' ", after.display());
        }
        let wrapper = dir.join("ritz-get");
        let script = format!(
            "#!/bin/sh\n\
             [ \"$1\" = --probe ] && exit 0\n\
             export {extra}FAKE_RITZ_GET_FIXTURE='{l}' FAKE_RITZ_GET_OFFLINE_FIXTURE='{l}' FAKE_RITZ_GET_LOG='{log}' \\\n\
             FAKE_RITZ_GET_SCENARIO='{scenario}' FAKE_RITZ_GET_DELAY=0 FAKE_RITZ_GET_DIR='{d}/clones'\n\
             {more_export}exec '{FAKE}' \"$@\"\n",
            l = listing.display(),
            log = log.display(),
            d = dir.display(),
            more_export = if more.is_empty() { String::new() } else { format!("export {}\n", more.replace("{DIR}", &dir.display().to_string())) },
        );
        std::fs::write(&wrapper, script).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // A concurrent fork in another test thread can briefly hold the fresh file open for
        // writing ("Text file busy"): wait until it executes.
        let mut ok = false;
        for _ in 0..200 {
            if std::process::Command::new(&wrapper).arg("--probe").status().is_ok_and(|s| s.success()) {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ok, "cannot execute {}", wrapper.display());

        let mut app = App::build(None);
        app.bin = Some(wrapper.to_string_lossy().into_owned());
        Rig { app, ctx: egui::Context::default(), dir, log, listing }
    }

    /// `App::update`'s loop without drawing, until nothing is queued or running.
    fn drive(&mut self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            for ev in self.app.runner.poll() {
                self.app.handle_event(ev);
            }
            self.app.pump(&self.ctx);
            if self.app.in_flight.is_none() && self.app.queue.is_empty() && !self.app.runner.is_busy() {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "the app did not go idle: queue {:?}", self.app.queue);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn start(&mut self) {
        self.app.begin_startup();
        self.drive();
    }

    /// What a pane's click does, then run to idle.
    fn click(&mut self, action: Action) {
        self.app.dispatch(&self.ctx, action);
        self.drive();
    }

    fn calls(&self) -> Vec<Call> {
        parse_log(&std::fs::read_to_string(&self.log).unwrap_or_default())
    }

    fn find(&self, name: &str) -> Entry {
        self.app.state.listing.entries().find(|e| e.name == name).unwrap_or_else(|| panic!("no {name} in the listing")).clone()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn phase(r: &Rig) -> Option<RunPhase> {
    r.app.state.run.as_ref().map(|x| x.phase.clone())
}

/// Poll / pump (no sleeping rig `drive`) until `cond` holds.
fn run_until(r: &mut Rig, cond: impl Fn(&AppState) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        for ev in r.app.runner.poll() {
            r.app.handle_event(ev);
        }
        r.app.pump(&r.ctx);
        if cond(&r.app.state) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "condition never held");
        std::thread::sleep(Duration::from_millis(1));
    }
}

const STARTUP: [&str; 4] = ["dir", "list --offline --porcelain --data", "list --porcelain --data", "shortcuts"];

#[test]
fn startup_runs_offline_list_then_online_list() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    let calls = r.calls();
    assert_eq!(argvs(&calls), STARTUP);
    // `list --offline --porcelain` strictly before `list --porcelain`.
    let pos = |a: &str| calls.iter().position(|c| c.argv == a).unwrap();
    assert!(pos("list --offline --porcelain --data") < pos("list --porcelain --data"));
    assert!(calls.iter().all(|c| c.exit == 0));
    assert_call_invariants(&calls);

    let s = &r.app.state;
    assert!(s.listing.entries().count() >= 17, "the full fixture is loaded");
    assert!(s.checked_at.is_some() && !s.loading && !s.busy);
    assert_eq!(s.dir, Some(r.dir.join("clones")), "`dir` output read");
    assert!(s.shortcuts_dir.is_some());
    assert_eq!(s.refused, None);
    assert!(!s.ritz_get_missing);
    assert!(s.actions_enabled());
}

/// Every action the panes can push, once, with the argv it must produce and the refresh calls the
/// app adds. Checks env, `--yes`, sudo, exit codes and no-overlap over the whole log.
#[test]
fn every_action_makes_the_right_call() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();

    // The shortcut actions come from the action list, driven by column 17.
    let off = r.find("ccorral");
    let on = r.find("yt-dlp-gui");
    assert_eq!(off.shortcut, ShortcutState::Off);
    assert_eq!(on.shortcut, ShortcutState::On);
    let shortcut_of = |e: &Entry| {
        side::offered_actions(&r.app.state, e).into_iter().find(|a| matches!(a, Action::Shortcut { .. }))
    };
    let create = shortcut_of(&off).expect("`off` offers a shortcut action");
    let remove = shortcut_of(&on).expect("`on` offers a shortcut action");
    assert_eq!(create, Action::Shortcut { name: "ccorral".into(), remove: false });
    assert_eq!(remove, Action::Shortcut { name: "yt-dlp-gui".into(), remove: true });

    const REFRESH: &str = "list --porcelain --data";
    let mut expected: Vec<String> = STARTUP.iter().map(|s| (*s).to_owned()).collect();
    let mut step = |r: &mut Rig, a: Action, argv: &[&str]| {
        r.click(a);
        expected.extend(argv.iter().map(|s| (*s).to_owned()));
    };
    step(&mut r, Action::Install { name: "ritz".into(), branch: None }, &["install ritz", REFRESH]);
    step(&mut r, Action::Install { name: "ForzaTelemetryV3Launcher".into(), branch: Some("dev".into()) }, &["install ForzaTelemetryV3Launcher --branch dev", REFRESH]);
    step(&mut r, Action::Repair { name: "ts6tray".into() }, &["install ts6tray", REFRESH]);
    step(&mut r, Action::Update { name: None }, &["update --yes", REFRESH]);
    step(&mut r, Action::Update { name: Some("ts6tray".into()) }, &["update ts6tray --yes", REFRESH]);
    step(&mut r, Action::Remove { name: "old-experiment".into() }, &["remove old-experiment", REFRESH]);
    step(&mut r, Action::Switch { name: "ts6tray".into(), branch: "dev".into() }, &["switch ts6tray dev", REFRESH]);
    step(&mut r, Action::Reset { name: "geofinder".into() }, &["reset geofinder --yes", REFRESH]);
    step(&mut r, Action::Hold { name: "ccorral".into() }, &["hold ccorral", REFRESH]);
    step(&mut r, Action::Unhold { name: "yt-dlp-gui".into() }, &["unhold yt-dlp-gui", REFRESH]);
    step(&mut r, Action::DeleteData { name: "ritz".into() }, &["delete-data ritz --yes", REFRESH]);
    step(&mut r, create, &["shortcut ccorral", REFRESH]);
    step(&mut r, remove, &["shortcut yt-dlp-gui --remove", REFRESH]);
    step(&mut r, Action::Open { name: "walters-knaggertreff".into() }, &["open walters-knaggertreff"]);
    step(&mut r, Action::Branches { name: "ts6tray".into() }, &["branches ts6tray"]);
    step(&mut r, Action::CheckForUpdates, &[REFRESH]);
    step(&mut r, Action::SetDir("/srv/ritz".into()), &["dir /srv/ritz --yes", "dir", REFRESH]);
    step(&mut r, Action::SetShortcutsDir("/srv/apps".into()), &["shortcuts /srv/apps", "shortcuts", REFRESH]);

    let calls = r.calls();
    assert_eq!(argvs(&calls), expected.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(calls.iter().all(|c| c.exit == 0), "{calls:?}");
    assert_call_invariants(&calls);
    // Spot checks on the exact env of the sensitive ones.
    let call = |a: &str| calls.iter().find(|c| c.argv == a).unwrap_or_else(|| panic!("no call {a}"));
    assert_eq!(call("install ritz").sudo, "pkexec");
    assert_eq!(call("switch ts6tray dev").sudo, "pkexec");
    assert_eq!(call("remove old-experiment").sudo, "pkexec");
    assert_eq!(call("reset geofinder --yes").sudo, "pkexec");
    assert_eq!(call("update --yes").sudo, "-");
    assert_eq!(call("shortcut ccorral").sudo, "-");
    assert_eq!(call("delete-data ritz --yes").sudo, "-");
    assert_eq!(call("dir /srv/ritz --yes").sudo, "-");
    assert_eq!(r.app.state.branches["ts6tray"].len(), 3);
    assert!(r.app.state.actions_enabled());
}

/// Column 17: `off` -> `shortcut NAME`, `on` -> `shortcut NAME --remove`, `-` -> no shortcut action
/// in the action list (for every entry of the fixture, websites and unlisted included).
#[test]
fn shortcut_action_follows_column_17() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    let mut seen = [0usize; 3];
    for e in r.app.state.listing.entries() {
        let sc: Vec<Action> = side::offered_actions(&r.app.state, e)
            .into_iter()
            .filter(|a| matches!(a, Action::Shortcut { .. }))
            .collect();
        match e.shortcut {
            ShortcutState::Off => {
                seen[0] += 1;
                assert_eq!(sc, [Action::Shortcut { name: e.name.clone(), remove: false }], "{}", e.id());
            }
            ShortcutState::On => {
                seen[1] += 1;
                assert_eq!(sc, [Action::Shortcut { name: e.name.clone(), remove: true }], "{}", e.id());
            }
            ShortcutState::None => {
                seen[2] += 1;
                assert!(sc.is_empty(), "{}: shortcut `-` must offer no shortcut action, got {sc:?}", e.id());
            }
        }
    }
    assert!(seen.iter().all(|n| *n > 0), "the fixture covers off/on/-: {seen:?}");

    // And the clicks reach the CLI as the documented argv.
    for (name, argv) in [("ccorral", "shortcut ccorral"), ("yt-dlp-gui", "shortcut yt-dlp-gui --remove"), ("walters-knaggertreff", "shortcut walters-knaggertreff --remove"), ("cs2-multimap", "shortcut cs2-multimap")] {
        let e = r.find(name);
        let a = side::offered_actions(&r.app.state, &e).into_iter().find(|a| matches!(a, Action::Shortcut { .. })).unwrap();
        r.click(a);
        assert!(r.calls().iter().any(|c| c.argv == argv), "{argv} not called");
    }
    assert_call_invariants(&r.calls());
}

// ---- exit codes -> UI -------------------------------------------------------------------------

fn install_ritz() -> Action {
    Action::Install { name: "ritz".into(), branch: None }
}

fn update_ts6tray() -> Action {
    Action::Update { name: Some("ts6tray".into()) }
}

/// Exit 1: a FAILED line in the log (red), the buttons are back.
#[test]
fn exit_1_shows_a_failed_log_line() {
    for action in [install_ritz(), update_ts6tray(), Action::Update { name: None }] {
        let mut r = Rig::new("failed", "full.porcelain");
        r.start();
        r.click(action.clone());
        let s = &r.app.state;
        assert!(
            s.log.iter().any(|l| l.kind == LogKind::Err && l.text.contains("FAILED")),
            "{action:?}: no red FAILED line in {:?}",
            s.log
        );
        assert!(s.log.iter().any(|l| l.text.contains("your installed version is unchanged and still works")));
        assert!(s.badges.is_empty(), "{action:?}: exit 1 sets no badge");
        assert_eq!(s.modal, Some(Modal::Run), "{action:?}: the failure is shown in the progress dialog");
        assert!(matches!(phase(&r), Some(RunPhase::Failed { .. })), "{action:?}: {:?}", phase(&r));
        let s = &r.app.state;
        assert!(!s.busy && s.actions_enabled());
        let calls = r.calls();
        assert_eq!(calls.iter().filter(|c| c.exit == 1).count(), 1, "{calls:?}");
        assert_call_invariants(&calls);
    }
}

/// Exit 3 with a `deps:` line: "Missing tools" in the progress dialog with the merged command (Copy command shown) and a
/// NEEDS badge per missing tool.
#[test]
fn exit_3_shows_the_deps_modal_with_command() {
    for (action, name, id) in [(install_ritz(), "ritz", "Software/ritz"), (update_ts6tray(), "ts6tray", "Helpers/ts6tray")] {
        let mut r = Rig::new("deps", "full.porcelain");
        r.start();
        r.click(action.clone());
        let s = &r.app.state;
        assert_eq!(s.modal, Some(Modal::Run), "{action:?}");
        assert_eq!(s.run.as_ref().and_then(|x| x.name.as_deref()), Some(name), "{action:?} ({id})");
        match phase(&r) {
            Some(RunPhase::Deps { missing, command }) => {
                assert_eq!(command.as_deref(), Some("sudo pacman -S --needed cmake pkgconf"), "{action:?}");
                assert_eq!(missing.len(), 2, "{missing:?}");
                assert!(missing[0].starts_with("missing: cmake") && missing[1].starts_with("missing: pkg-config"));
            }
            m => panic!("{action:?}: expected the Missing tools state, got {m:?}"),
        }
        let want = Badge::Needs(vec!["cmake".into(), "pkg-config".into()]);
        assert_eq!(s.badges.get(name), Some(&want), "{action:?}: one NEEDS pill per missing tool");
        assert!(!s.busy && s.actions_enabled(), "buttons are enabled again");
        assert_eq!(r.calls().iter().filter(|c| c.exit == 3).count(), 1);
        assert_call_invariants(&r.calls());
    }
}

/// Exit 3 on an unknown distro: no `deps:` line, so the modal carries the raw `missing:` lines and
/// no command (the modal hides Copy command).
#[test]
fn exit_3_without_deps_line_shows_raw_missing_lines() {
    let mut r = Rig::new("deps-nodeps", "full.porcelain");
    r.start();
    r.click(install_ritz());
    match phase(&r) {
        Some(RunPhase::Deps { missing, command }) => {
            assert_eq!(command, None, "no deps: line -> no command -> no Copy command");
            assert!(missing.iter().all(|l| l.starts_with("missing: ")), "{missing:?}");
            assert_eq!(missing.len(), 2);
        }
        m => panic!("expected the Missing tools state, got {m:?}"),
    }
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    assert_eq!(r.app.state.badges.get("ritz"), Some(&Badge::Needs(vec!["cmake".into(), "pkg-config".into()])));
    assert!(r.app.state.actions_enabled());
    assert_call_invariants(&r.calls());
}

/// Exit 4: SETUP NEEDED badge + the "run it in a terminal" hint, shown in the progress dialog.
#[test]
fn exit_4_shows_the_setup_needed_badge() {
    for (action, name) in [(install_ritz(), "ritz"), (update_ts6tray(), "ts6tray")] {
        let mut r = Rig::new("needs-root", "full.porcelain");
        r.start();
        r.click(action.clone());
        let s = &r.app.state;
        assert_eq!(s.badges.get(name), Some(&Badge::SetupNeeded), "{action:?}");
        assert!(s.log.iter().any(|l| l.text.contains(&format!("run `ritz-get install {name}` in a terminal"))), "{:?}", s.log);
        assert_eq!(phase(&r), Some(RunPhase::Setup));
        assert_eq!(s.modal, Some(Modal::Run));
        assert!(!s.busy && s.actions_enabled());
        // The pill the card/side draw for it:
        let e = r.find(name);
        let pills = crate::ui::pills::pills(s, &e);
        assert!(pills.iter().any(|p| p.text == "Setup needed"), "{pills:?}");
        assert_eq!(r.calls().iter().filter(|c| c.exit == 4).count(), 1);
        assert_call_invariants(&r.calls());
    }
}

/// Exit 5: the busy message in the log, buttons enabled again, and no refresh call.
#[test]
fn exit_5_shows_the_busy_message_and_reenables_actions() {
    let mut r = Rig::new("busy", "full.porcelain");
    r.start();
    let before = r.calls().len();
    r.click(install_ritz());
    let s = &r.app.state;
    let busy: Vec<_> = s.log.iter().filter(|l| l.text.contains("another ritz-get is running")).collect();
    assert_eq!(busy.len(), 1, "exactly one busy line (the CLI's, not doubled by the GUI): {:?}", s.log);
    assert!(busy[0].kind == LogKind::Err && busy[0].text == "busy: another ritz-get is running", "{:?}", s.log);
    assert!(!s.busy && s.running.is_none() && s.actions_enabled(), "buttons are enabled again");
    assert!(s.badges.is_empty());
    assert_eq!(phase(&r), Some(RunPhase::Busy), "the busy message is in the dialog's result area");
    let calls = r.calls();
    assert_eq!(calls.len(), before + 1, "busy refreshes nothing: {calls:?}");
    assert_eq!(calls.last().unwrap().argv, "install ritz");
    assert_eq!(calls.last().unwrap().exit, 5);
    assert_call_invariants(&calls);
    // And the next action goes through once ritz-get is free again.
    r.click(Action::CloseModal);
    r.click(Action::CheckForUpdates);
    assert_eq!(r.calls().last().unwrap().argv, "list --porcelain --data");
}

/// `bad-version.porcelain` (`#porcelain 2`): refused, nothing else runs, and the dialog's
/// "Update ritz-get-gui" works: it runs `update ritz-get-gui --yes`, raises the restart bar and
/// starts over (here with a fixture the new version understands).
#[test]
fn porcelain_2_is_refused_and_the_update_path_works() {
    let mut r = Rig::new("ok", "bad-version.porcelain");
    r.start();
    assert_eq!(r.app.state.refused, Some(2));
    assert!(r.app.state.listing.sections.is_empty());
    assert!(!r.app.state.actions_enabled());
    // First startup: the online list is dropped after the refusal.
    assert_eq!(argvs(&r.calls()), ["dir", "list --offline --porcelain --data", "shortcuts"]);

    // A command that is not the self-update is dropped.
    r.click(Action::Update { name: None });
    r.click(Action::Hold { name: "x".into() });
    assert_eq!(r.calls().len(), 3, "nothing else runs while refused");

    // The dialog's button: `Action::Update { name: Some("ritz-get-gui") }`.
    std::fs::copy(format!("{FIXTURES}/full.porcelain"), &r.listing).unwrap(); // "the new GUI understands it"
    r.click(Action::Update { name: Some("ritz-get-gui".into()) });
    let calls = r.calls();
    assert_eq!(
        argvs(&calls),
        [
            "dir",
            "list --offline --porcelain --data",
            "shortcuts",
            "update ritz-get-gui --yes",
            "dir",
            "list --offline --porcelain --data",
            "list --porcelain --data",
            "shortcuts",
        ]
    );
    assert_call_invariants(&calls);
    let s = &r.app.state;
    assert_eq!(s.refused, None, "startup re-ran");
    assert!(s.restart, "the self-update raises the restart bar");
    assert!(s.listing.entries().count() >= 17 && s.actions_enabled());
}

/// A pane that never loaded anything can still say "ritz-get not found" (spawn failure), and Retry
/// starts over.
#[test]
fn missing_binary_shows_the_banner_state_and_retry_recovers() {
    let mut r = Rig::new("ok", "full.porcelain");
    let good = r.app.bin.clone();
    r.app.bin = Some(r.dir.join("no-such-ritz-get").to_string_lossy().into_owned());
    r.start();
    assert!(r.app.state.ritz_get_missing && !r.app.state.actions_enabled());
    assert!(r.app.state.log.iter().any(|l| l.text.contains("ritz-get not found")));
    r.app.bin = good;
    r.click(Action::Retry);
    assert!(!r.app.state.ritz_get_missing && r.app.state.actions_enabled());
    assert_eq!(argvs(&r.calls()), STARTUP);
}

// ---- D47: "Add it to the start menu?" as the Done state of a fresh install ------------------------

/// Install `ritz` (not installed, shortcut `-` before); the refreshed listing says `col17`.
fn install_then_list(col17: &str, action: Action) -> Rig {
    let mut r = Rig::new_with("ok", "full.porcelain", Some(("ritz", col17)));
    r.start();
    r.click(action);
    r
}

/// Refreshed listing says `off`: the progress dialog ends in the 2a question; Add runs `shortcut ritz`
/// after install + refresh.
#[test]
fn install_with_off_shortcut_asks_and_add_creates_it() {
    for action in [install_ritz(), Action::Install { name: "ritz".into(), branch: Some("dev".into()) }] {
        let mut r = install_then_list("off", action.clone());
        assert_eq!(r.app.state.modal, Some(Modal::Run), "{action:?}");
        assert_eq!(phase(&r), Some(RunPhase::Done { menu: true }), "{action:?}");
        assert_eq!(r.app.state.run.as_ref().map(|x| x.verb), Some(Verb::Install));
        assert!(r.app.state.actions_enabled());
        // What the dialog's [Add to start menu] pushes.
        r.click(Action::Shortcut { name: "ritz".into(), remove: false });
        r.click(Action::CloseModal);
        let calls = r.calls();
        let argv = argvs(&calls);
        let n = argv.len();
        assert_eq!(argv[n - 4], if matches!(action, Action::Install { branch: Some(_), .. }) { "install ritz --branch dev" } else { "install ritz" });
        assert_eq!(argv[n - 3..], ["list --porcelain --data", "shortcut ritz", "list --porcelain --data"]);
        assert!(r.app.state.modal.is_none() && r.app.state.run.is_none());
        assert_call_invariants(&calls);
    }
}

/// [Not now]: no `shortcut` call at all.
#[test]
fn install_with_off_shortcut_not_now_makes_no_call() {
    let mut r = install_then_list("off", install_ritz());
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: true }));
    r.click(Action::CloseModal);
    assert!(r.app.state.modal.is_none() && r.app.state.run.is_none());
    let calls = r.calls();
    assert!(!calls.iter().any(|c| c.argv.starts_with("shortcut ")), "{:?}", argvs(&calls));
    assert_eq!(argvs(&calls)[STARTUP.len()..], ["install ritz", "list --porcelain --data"]);
}

/// Column 17 `-` (no .desktop) or already `on`: never asked; the Done state is just the result.
#[test]
fn install_without_off_shortcut_does_not_ask() {
    for col17 in ["-", "on"] {
        let r = install_then_list(col17, install_ritz());
        assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }), "column 17 = {col17}");
        assert!(r.app.offer_shortcut.is_none());
        assert!(!r.calls().iter().any(|c| c.argv.starts_with("shortcut ")));
    }
}

/// Repair is `install NAME` on an installed program: never asked, even if the listing says `off`.
#[test]
fn repair_does_not_ask() {
    let mut r = Rig::new_with("ok", "full.porcelain", Some(("ts6tray", "off")));
    r.start();
    r.click(Action::Repair { name: "ts6tray".into() });
    assert_eq!(argvs(&r.calls())[STARTUP.len()..], ["install ts6tray", "list --porcelain --data"]);
    assert_eq!(r.find("ts6tray").shortcut, ShortcutState::Off, "the refreshed listing does say off");
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
    assert_eq!(r.app.state.run.as_ref().map(|x| x.verb), Some(Verb::Repair));
}

/// Update / switch / reset / remove / a failed install never ask either.
#[test]
fn other_commands_and_failed_installs_do_not_ask() {
    let mut r = Rig::new_with("ok", "full.porcelain", Some(("ccorral", "off")));
    r.start();
    for a in [
        Action::Update { name: None },
        Action::Switch { name: "ts6tray".into(), branch: "dev".into() },
        Action::Reset { name: "geofinder".into() },
        Action::Remove { name: "old-experiment".into() },
    ] {
        r.click(a);
        assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
        r.click(Action::CloseModal);
        assert!(r.app.state.modal.is_none() && r.app.state.run.is_none());
    }
    let mut r = Rig::new_with("failed", "full.porcelain", Some(("ritz", "off")));
    r.start();
    r.click(install_ritz());
    assert!(matches!(phase(&r), Some(RunPhase::Failed { .. })) && r.app.offer_shortcut.is_none());
}

// ---- the progress dialog ----------------------------------------------------------------------

/// Install opens the dialog the moment the command starts, shows the newest log line while it runs,
/// and ends in Done with the start-menu question (column 17 `off`).
#[test]
fn install_opens_the_progress_modal_and_finishes_with_the_question() {
    let mut r = Rig::new_env("ok", "full.porcelain", Some(("ritz", "off")), "FAKE_RITZ_GET_DELAY=0.03");
    r.start();
    assert!(r.app.state.modal.is_none() && r.app.state.run.is_none(), "queries open nothing");
    let seen = click_watch(&mut r, install_ritz(), |s| {
        (s.modal.clone(), s.run.as_ref().map(|x| x.phase.clone()), s.run_latest().map(|l| l.text.clone()))
    });
    assert_eq!(seen[0].0, Some(Modal::Run), "{seen:?}");
    assert_eq!(seen[0].1, Some(RunPhase::Running));
    assert!(seen.iter().any(|(_, p, l)| *p == Some(RunPhase::Running) && l.as_deref() == Some("Building ritz ...")), "{seen:?}");
    let last = seen.last().unwrap();
    assert_eq!((last.0.clone(), last.1.clone()), (Some(Modal::Run), Some(RunPhase::Done { menu: true })));
    // The newest line stays the command's last output; the dialog's log is the run's own lines.
    let lines: Vec<&str> = r.app.state.run_lines().iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, ["$ ritz-get install ritz", "Cloning ritz (main) ...", "Building ritz ...", "Installed ritz"]);
}

/// Update opens it too, and finishes without the question; Done stays until the user closes it.
#[test]
fn update_opens_the_progress_modal_and_finishes_without_the_question() {
    let mut r = Rig::new_env("ok", "full.porcelain", Some(("ts6tray", "off")), "FAKE_RITZ_GET_DELAY=0.02");
    r.start();
    let seen = click_watch(&mut r, update_ts6tray(), |s| s.run.as_ref().map(|x| x.phase.clone()));
    let seen: Vec<RunPhase> = seen.into_iter().flatten().collect();
    assert_eq!(seen, [RunPhase::Running, RunPhase::Done { menu: false }]);
    let run = r.app.state.run.clone().unwrap();
    assert_eq!(run.verb, Verb::Update);
    assert_eq!(run.change, Some(("2026-10-01 abc1234".into(), "2026-10-20 def5678".into())));
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    r.click(Action::CloseModal);
    assert!(r.app.state.modal.is_none() && r.app.state.run.is_none());
}

/// Run `action` and hide the dialog as soon as the command has printed something.
fn start_in_background(r: &mut Rig, action: Action) {
    r.app.dispatch(&r.ctx, action);
    run_until(r, |s| s.run_latest().is_some());
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    r.app.dispatch(&r.ctx, Action::CloseModal); // Esc / backdrop while it runs
    assert!(r.app.state.modal.is_none(), "Run in background closes the dialog");
    assert!(r.app.state.run.as_ref().is_some_and(|x| x.running()), "... and the run goes on");
}

/// Run in background: the dialog goes, the way back is offered; a successful update finishes silently.
#[test]
fn background_update_finishes_silently() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, update_ts6tray());
    assert_eq!(r.app.state.reopen_label().as_deref(), Some("Updating TeamSpeak 6 Tray\u{2026}"));
    // the card keeps its bar meanwhile
    assert!(r.app.state.progress.contains_key("ts6tray"));
    r.drive();
    assert!(r.app.state.modal.is_none() && r.app.state.run.is_none(), "a plain success needs nobody");
    assert_eq!(r.app.state.reopen_label(), None);
    assert_call_invariants(&r.calls());
}

/// A backgrounded failure reopens the dialog; so does the start-menu question and missing tools.
#[test]
fn background_run_reopens_when_it_needs_the_user() {
    let mut r = Rig::new_env("failed", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, update_ts6tray());
    r.drive();
    assert_eq!(r.app.state.modal, Some(Modal::Run), "a failure comes back");
    assert!(matches!(phase(&r), Some(RunPhase::Failed { .. })));

    let mut r = Rig::new_env("deps", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, install_ritz());
    r.drive();
    assert_eq!(r.app.state.modal, Some(Modal::Run), "missing tools come back");
    assert!(matches!(phase(&r), Some(RunPhase::Deps { .. })));

    let mut r = Rig::new_env("ok", "full.porcelain", Some(("ritz", "off")), "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, install_ritz());
    r.drive();
    assert_eq!(r.app.state.modal, Some(Modal::Run), "the start-menu question comes back");
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: true }));

    // ... and with nothing to ask, a fresh install finishes silently too
    let mut r = Rig::new_env("ok", "full.porcelain", Some(("ritz", "-")), "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, install_ritz());
    r.drive();
    assert!(r.app.state.modal.is_none() && r.app.state.run.is_none());
}

/// While the dialog is hidden a click on the reopen button shows it again.
#[test]
fn hidden_run_can_be_reopened() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.05");
    r.start();
    start_in_background(&mut r, install_ritz());
    r.app.dispatch(&r.ctx, Action::ShowModal(Modal::Run));
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    assert_eq!(r.app.state.reopen_label(), None, "nothing to offer while it is showing");
    r.drive();
}

/// The card's "Installing..." bar brings a backgrounded install's dialog back with its log; Stop kills
/// the command, the run reads as stopped (not done), and the listing is re-read.
#[test]
fn hidden_install_reopens_from_its_card_and_stop_cancels_it() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.2");
    r.start();
    start_in_background(&mut r, install_ritz());
    assert!(r.app.state.run_hidden(), "the cards' bars are clickable now");
    let lines = r.app.state.run_lines().len();
    r.app.dispatch(&r.ctx, Action::ShowModal(Modal::Run)); // what a click on the bar sends
    let s = &r.app.state;
    assert_eq!((s.modal.clone(), s.run_hidden()), (Some(Modal::Run), false));
    assert!(s.run.as_ref().is_some_and(|x| x.running()) && s.run_lines().len() >= lines, "same run, log kept");

    r.app.dispatch(&r.ctx, Action::StopRun);
    assert!(r.app.state.run.as_ref().is_some_and(|x| x.stopping), "the button reads Stopping...");
    r.drive();
    let s = &r.app.state;
    assert_eq!(phase(&r), Some(RunPhase::Stopped));
    assert_eq!(s.modal, Some(Modal::Run), "the dialog the user stopped it from stays");
    assert!(!s.busy && s.actions_enabled() && !s.progress.contains_key("ritz"), "idle again, no FAILED bar");
    assert!(s.log.iter().any(|l| l.kind == LogKind::Note && l.text.contains("was stopped")));
    assert!(r.calls().iter().any(|c| c.argv.starts_with("list")), "re-listed after the stop: {:?}", r.calls());
    r.click(Action::CloseModal);
    assert!(r.app.state.run.is_none());
}

/// Update all opens the list dialog; its rows follow the result lines (the card bars) and the
/// header counts the finished ones.
#[test]
fn update_all_modal_rows_follow_result_lines() {
    let env = "FAKE_RITZ_GET_DELAY=0.03 FAKE_RITZ_GET_UPDATE_ALL_FILE={DIR}/update-all.txt";
    let mut r = Rig::new_env("ok", "full.porcelain", None, env);
    std::fs::write(
        r.dir.join("update-all.txt"),
        "ritz-get-gui: up to date\nbar: held (ritz-get unhold bar)\nts6tray: updated 2026-10-20 a596443 -> 2026-10-22 b12c3d4\n1 updated, 1 skipped, 0 failed\n",
    )
    .unwrap();
    r.start();
    let seen = click_watch(&mut r, Action::Update { name: None }, |s| {
        (s.modal.clone(), s.run_counts(), s.run_position(), s.run.as_ref().map(|x| x.phase.clone()))
    });
    assert_eq!(seen[0].0, Some(Modal::Run));
    let counts: Vec<(usize, usize)> = seen.iter().map(|s| s.1).collect();
    assert!(counts.starts_with(&[(0, 2)]) && counts.contains(&(1, 2)), "{counts:?}");
    assert_eq!(seen.iter().find(|s| s.1 == (1, 2)).map(|s| s.2), Some((2, 2)), "header: updating 2 of 2 once one is finished");
    let s = &r.app.state;
    assert_eq!(s.run_counts(), (2, 2));
    assert_eq!(s.run.as_ref().map(|x| x.order.clone()), Some(vec!["ritz-get-gui".to_owned(), "ts6tray".to_owned()]));
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
    assert_eq!(s.modal, Some(Modal::Run), "the bars (rows) stay while the dialog shows");
    assert_eq!(crate::ui::modals::tally(s).0, "1 updated, 1 up to date");
    // The rows are the bars: they outlive the usual fade while the dialog is open ...
    let mut late = std::mem::take(&mut r.app.state);
    assert_eq!(late.expire_progress(Instant::now() + PROGRESS_FADE * 10), None);
    assert_eq!(late.progress.len(), 2);
    // ... and go once it is closed.
    late.apply_ui_action(Action::CloseModal);
    late.expire_progress(Instant::now() + PROGRESS_FADE * 10);
    assert!(late.progress.is_empty() && late.run.is_none());
}

/// Update all with a failing program ends in the finished list with the failure (exit 1).
#[test]
fn update_all_failure_is_listed_and_reopens() {
    let mut r = Rig::new_env("failed", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.02");
    r.start();
    start_in_background(&mut r, Action::Update { name: None });
    r.drive();
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    assert!(matches!(phase(&r), Some(RunPhase::Failed { .. })));
    assert!(r.app.state.run.as_ref().unwrap().is_all());
}

/// A read-only query in flight (the startup lists, `branches`) does not disable the actions, and
/// an action clicked meanwhile queues behind it: `branches NAME`, then `install NAME`, no overlap.
#[test]
fn queries_do_not_disable_actions_and_a_click_queues_behind_them() {
    let mut r = Rig::new("ok", "full.porcelain");
    // Startup: dir / list / shortcuts are queries too.
    r.app.begin_startup();
    r.app.pump(&r.ctx);
    assert!(r.app.state.busy, "a query runs");
    assert!(r.app.state.actions_enabled(), "... but the cards stay enabled");
    r.drive();

    // The branch ComboBox opens: `branches ritz` runs.
    r.app.dispatch(&r.ctx, Action::Branches { name: "ritz".into() });
    r.app.pump(&r.ctx);
    assert!(r.app.in_flight.is_some() && r.app.state.busy);
    assert!(r.app.state.actions_enabled(), "install buttons stay enabled while a query runs");
    assert!(r.app.state.run.is_none() && r.app.state.progress.is_empty() && r.app.state.modal.is_none(), "no dialog or bar for a query");
    // Clicking Install now is queued, not dropped.
    r.app.dispatch(&r.ctx, install_ritz());
    assert_eq!(r.app.queue.len(), 1, "{:?}", r.app.queue);
    assert!(!r.app.state.actions_enabled(), "a mutating command is waiting: now they are off");
    r.drive();
    let calls = r.calls();
    assert_eq!(argvs(&calls)[STARTUP.len()..], ["branches ritz", "install ritz", "list --porcelain --data"]);
    assert_call_invariants(&calls);
    assert_eq!(r.app.state.modal, Some(Modal::Run), "the queued install opened its dialog when it started");
    assert!(r.app.state.actions_enabled());

    // A mutating command running DOES disable them, and Check for updates keeps its own gate.
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.2");
    r.start();
    r.app.dispatch(&r.ctx, install_ritz());
    r.app.pump(&r.ctx);
    assert!(r.app.state.busy && !r.app.state.actions_enabled());
    r.drive();
}

// ---- per-card progress bars --------------------------------------------------------------

/// `click`, but watching: `snap` is taken after every runner event and every pump; consecutive
/// equal snapshots are merged. The fake streams slowly (`FAKE_RITZ_GET_DELAY`), so every state
/// between two result lines is observed.
fn click_watch<T: PartialEq>(r: &mut Rig, a: Action, snap: impl Fn(&AppState) -> T) -> Vec<T> {
    let mut seen: Vec<T> = Vec::new();
    let note = |s: &AppState, seen: &mut Vec<T>| {
        let v = snap(s);
        if seen.last() != Some(&v) {
            seen.push(v);
        }
    };
    r.app.dispatch(&r.ctx, a);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        for ev in r.app.runner.poll() {
            r.app.handle_event(ev);
            note(&r.app.state, &mut seen);
        }
        r.app.pump(&r.ctx);
        note(&r.app.state, &mut seen);
        if r.app.in_flight.is_none() && r.app.queue.is_empty() && !r.app.runner.is_busy() {
            return seen;
        }
        assert!(std::time::Instant::now() < deadline, "the app did not go idle");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn phase_of(s: &AppState, name: &str) -> Option<Progress> {
    s.progress.get(name).map(|c| c.phase.clone())
}

/// A single install: its card is Running while the command streams, Done once it exited 0, and the
/// bar goes (after the refresh and PROGRESS_FADE), not before.
#[test]
fn single_install_is_running_then_done_then_gone() {
    let mut r = Rig::new_env("ok", "full.porcelain", Some(("ritz", "off")), "FAKE_RITZ_GET_DELAY=0.03");
    r.start();
    assert!(r.app.state.progress.is_empty(), "nothing tracked at startup / after plain lists");
    let seen = click_watch(&mut r, Action::Install { name: "ritz".into(), branch: None }, |s| phase_of(s, "ritz"));
    let seen: Vec<Progress> = seen.into_iter().flatten().collect();
    assert_eq!(seen, [Progress::Running, Progress::Done]);

    r.click(Action::CloseModal); // the progress dialog keeps the bars while it shows
    let s = &mut r.app.state;
    let cp = &s.progress["ritz"];
    assert_eq!((cp.verb, cp.label()), (Verb::Install, "Done"));
    assert_eq!(s.progress.len(), 1, "only that card");
    assert!(cp.ended.is_some() && !s.busy && !s.loading, "the refresh is in");
    let now = Instant::now();
    assert!(s.expire_progress(now).is_some(), "still showing");
    assert!(s.progress.contains_key("ritz"));
    assert!(s.expire_progress(now + PROGRESS_FADE + Duration::from_millis(10)).is_none());
    assert!(s.progress.is_empty(), "back to the normal button");
}

/// The finished bar survives a slow refresh: while a `list` is queued/running nothing expires.
#[test]
fn finished_bar_waits_for_the_refresh() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    r.click(Action::Update { name: Some("ts6tray".into()) });
    r.click(Action::CloseModal); // the progress dialog keeps the bars while it shows
    let late = Instant::now() + PROGRESS_FADE * 5;
    r.app.state.loading = true;
    assert_eq!(r.app.state.expire_progress(late), None);
    assert!(r.app.state.progress.contains_key("ts6tray"), "list still in flight: keep the bar");
    r.app.state.loading = false;
    r.app.state.expire_progress(late);
    assert!(r.app.state.progress.is_empty());
}

/// Update all with the fake streaming `ritz-get-gui: up to date`, `bar: held`, `ts6tray: updated`:
/// both updatable cards start Queued/Running; each result line moves its card and promotes the next;
/// `bar` (no bar) and the other cards are untouched.
#[test]
fn update_all_walks_queued_running_result() {
    let env = "FAKE_RITZ_GET_DELAY=0.03 FAKE_RITZ_GET_UPDATE_ALL_FILE={DIR}/update-all.txt";
    let mut r = Rig::new_env("ok", "full.porcelain", None, env);
    std::fs::write(
        r.dir.join("update-all.txt"),
        "ritz-get-gui: up to date\nbar: held (ritz-get unhold bar)\nts6tray: updated 2026-10-20 a596443 -> 2026-10-22 b12c3d4\n1 updated, 1 skipped, 0 failed\n",
    )
    .unwrap();
    r.start();
    let set: Vec<String> = r.app.state.listing.entries().filter(|e| e.updatable()).map(|e| e.name.clone()).collect();
    assert_eq!(set, ["ritz-get-gui", "ts6tray"], "the fixture's Update all set");

    let seen = click_watch(&mut r, Action::Update { name: None }, |s| {
        (phase_of(s, "ritz-get-gui"), phase_of(s, "ts6tray"), s.progress.len())
    });
    let seen: Vec<_> = seen.into_iter().filter(|(a, _, _)| a.is_some()).collect();
    let up = Progress::Result(Outcome::Updated { from: "2026-10-20 a596443".into(), to: "2026-10-22 b12c3d4".into() });
    assert_eq!(
        seen,
        [
            (Some(Progress::Running), Some(Progress::Queued), 2),
            (Some(Progress::Result(Outcome::UpToDate)), Some(Progress::Running), 2),
            (Some(Progress::Result(Outcome::UpToDate)), Some(up), 2),
        ]
    );
    let s = &r.app.state;
    assert_eq!(s.progress["ritz-get-gui"].label(), "Up to date");
    assert_eq!(s.progress["ts6tray"].label(), "Updated");
    assert!(s.progress.values().all(|c| c.ended.is_some()));
    assert!(["bar", "geofinder", "yt-dlp-gui"].iter().all(|n| !s.progress.contains_key(*n)));
}

/// The built-in update-all output (`geofinder`, `bar`, ... none in the Update all set) leaves the
/// set's cards Queued -> Running -> Done at the end (no line of theirs, exit 0).
#[test]
fn update_all_without_matching_lines_ends_done() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.02");
    r.start();
    let seen = click_watch(&mut r, Action::Update { name: None }, |s| phase_of(s, "ts6tray"));
    let seen: Vec<Progress> = seen.into_iter().flatten().collect();
    assert_eq!(seen, [Progress::Queued, Progress::Done]);
    assert_eq!(r.app.state.progress["ritz-get-gui"].phase, Progress::Done);
}

/// The failed scenario: the FAILED line turns the card Failed (single install, then single update
/// replacing the old bar); the red bar stays until it fades.
#[test]
fn failed_command_marks_the_card_failed() {
    let mut r = Rig::new_env("failed", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.02");
    r.start();
    let seen = click_watch(&mut r, Action::Install { name: "ritz".into(), branch: None }, |s| phase_of(s, "ritz"));
    let seen: Vec<Progress> = seen.into_iter().flatten().collect();
    assert_eq!(seen, [Progress::Running, Progress::Failed]);
    assert_eq!(r.app.state.progress["ritz"].label(), "FAILED");
    assert_eq!(r.app.state.progress["ritz"].tone(), Tone::Danger);

    let seen = click_watch(&mut r, Action::Update { name: Some("ts6tray".into()) }, |s| {
        (phase_of(s, "ritz"), phase_of(s, "ts6tray"))
    });
    assert_eq!(seen.last(), Some(&(None, Some(Progress::Failed))), "the new run replaces the old bars");
    assert_eq!(r.app.state.progress["ts6tray"].verb, Verb::Update);
}

/// A busy refusal (exit 5) changed nothing: no bar is left behind.
#[test]
fn busy_refusal_leaves_no_bar() {
    let mut r = Rig::new_env("busy", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.01");
    r.start();
    r.click(Action::Update { name: None });
    assert!(r.app.state.progress.is_empty());
    r.click(Action::Install { name: "ritz".into(), branch: None });
    assert!(r.app.state.progress.is_empty());
}

// ---- ritz-get's #start / #progress lines ---------------------------------------------------

/// Run the fake CLI by hand (`RITZ_GET_GUI=1` unless `gui` is false) and give its stdout lines.
fn fake_stdout(args: &[&str], envs: &[(&str, &str)], gui: bool) -> Vec<String> {
    let mut c = std::process::Command::new(FAKE);
    c.args(args).env("FAKE_RITZ_GET_DELAY", "0").env_remove("RITZ_GET_GUI").env_remove("FAKE_RITZ_GET_NO_PROGRESS");
    if gui {
        c.env("RITZ_GET_GUI", "1");
    }
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.output().expect("the fake runs");
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

/// The fake prints what PORCELAIN.md "Progress lines" says: phase-1 results without markers, the apply
/// list with start / result / progress, M = number of names, the unknown-name pair, nothing without
/// RITZ_GET_GUI or with FAKE_RITZ_GET_NO_PROGRESS.
#[test]
fn fake_cli_prints_the_progress_lines() {
    let marks = |v: &[String]| -> Vec<String> { v.iter().filter(|l| l.starts_with('#')).cloned().collect() };
    let v = fake_stdout(&["install", "a", "b"], &[], true);
    assert_eq!(marks(&v), ["#start a", "#progress a 1/2", "#start b", "#progress b 2/2"]);
    let pos = |l: &str| v.iter().position(|x| x == l).unwrap();
    assert!(pos("#start a") < pos("Installed a") && pos("Installed a") < pos("#progress a 1/2"));

    let v = fake_stdout(&["install", "nosuch", "a"], &[], true);
    assert_eq!(marks(&v), ["#start nosuch", "#progress nosuch 1/2", "#start a", "#progress a 2/2"]);

    let v = fake_stdout(&["update", "--yes"], &[], true);
    assert_eq!(v[0], "Checking for updates ...");
    assert!(v[1..4].iter().all(|l| !l.starts_with('#')), "phase 1 settles held / skipped / up to date: {v:?}");
    assert_eq!(marks(&v), ["#start geofinder", "#progress geofinder 1/1"]);
    for args in [&["update", "ts6tray", "--yes"][..], &["reset", "ts6tray", "--yes"], &["switch", "ts6tray", "dev"]] {
        assert_eq!(marks(&fake_stdout(args, &[], true)), ["#start ts6tray", "#progress ts6tray 1/1"], "{args:?}");
    }
    let v = fake_stdout(&["install", "ritz"], &[("FAKE_RITZ_GET_SCENARIO", "failed")], true);
    assert_eq!(marks(&v), ["#start ritz", "#progress ritz 1/1"]);
    assert!(v.iter().position(|l| l == "ritz: FAILED (install.sh exit 1)") < v.iter().position(|l| l == "#progress ritz 1/1"), "{v:?}");

    assert!(marks(&fake_stdout(&["install", "a", "b"], &[], false)).is_empty(), "no RITZ_GET_GUI, no markers");
    assert!(marks(&fake_stdout(&["update", "--yes"], &[("FAKE_RITZ_GET_NO_PROGRESS", "1")], true)).is_empty());
}

/// A file for `FAKE_RITZ_GET_UPDATE_ALL_FILE`.
fn update_all_file(r: &Rig, lines: &[&str]) {
    std::fs::write(r.dir.join("update-all.txt"), lines.join("\n") + "\n").unwrap();
}

const UPDATED: &str = "updated 2026-10-20 a596443 -> 2026-10-22 b12c3d4";

/// Update all against a ritz-get with markers: ritz-get-gui is settled in phase 1 (final at once,
/// not counted), ts6tray is the only one applied: header and counts come from `#progress`.
#[test]
fn update_all_follows_the_markers() {
    let env = "FAKE_RITZ_GET_DELAY=0.03 FAKE_RITZ_GET_UPDATE_ALL_FILE={DIR}/update-all.txt";
    let mut r = Rig::new_env("ok", "full.porcelain", None, env);
    update_all_file(
        &r,
        &[
            "ritz-get-gui: up to date",
            "bar: held (ritz-get unhold bar)",
            "#start ts6tray",
            &format!("ts6tray: {UPDATED}"),
            "#progress ts6tray 1/1",
            "1 updated, 1 skipped, 0 failed",
        ],
    );
    r.start();
    let seen = click_watch(&mut r, Action::Update { name: None }, |s| {
        (phase_of(s, "ritz-get-gui"), phase_of(s, "ts6tray"), s.run_all_title(), s.run_counted())
    });
    let ts6 = r.app.state.display_of("ts6tray");
    assert_eq!(seen.first().unwrap().2, "Checking for updates\u{2026}", "phase 1: honest");
    // ritz-get-gui: final as soon as its phase-1 line is in, and stays that way
    let settled = seen.iter().position(|s| s.0 == Some(Progress::Result(Outcome::UpToDate))).expect("settled in phase 1");
    assert!(seen[settled..].iter().all(|s| s.0 == Some(Progress::Result(Outcome::UpToDate))));
    assert!(seen[..settled].iter().all(|s| s.2 == "Checking for updates\u{2026}"));
    // the running card is the `#start` one, the header names it until M is known, then N of M
    let running = seen.iter().position(|s| s.2 == format!("Updating {ts6}\u{2026}")).expect("named while M is unknown");
    assert!(seen[running].1 == Some(Progress::Running) && !seen[running].3);
    let counted = seen.iter().position(|s| s.2 == "Updating 1 of 1\u{2026}").expect("N of M after #progress");
    assert!(counted > running && seen[counted].3);
    let s = &r.app.state;
    assert_eq!(s.run_counts(), (1, 1), "the phase-1 program is not in M (the set has 2)");
    assert_eq!(s.progress["ts6tray"].label(), "Updated");
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
    assert_eq!(crate::ui::modals::tally(s).0, "1 updated, 1 up to date");
    assert!(s.log.iter().all(|l| !l.text.starts_with('#')), "machine lines stay out of the log (and Copy log)");
    assert!(s.log.iter().any(|l| l.text.starts_with("ts6tray: updated")));
}

/// ritz-get updated itself first: `1/1`, then (re-exec) `1/M`.
#[test]
fn update_all_self_update_runs_two_segments() {
    let env = "FAKE_RITZ_GET_DELAY=0.03 FAKE_RITZ_GET_UPDATE_ALL_FILE={DIR}/update-all.txt";
    let mut r = Rig::new_env("ok", "full.porcelain", None, env);
    update_all_file(
        &r,
        &[
            "#start ritz-get",
            &format!("ritz-get: {UPDATED}"),
            "#progress ritz-get 1/1",
            "bar: held (ritz-get unhold bar)",
            "#start ritz-get-gui",
            &format!("ritz-get-gui: {UPDATED}"),
            "#progress ritz-get-gui 1/2",
            "#start ts6tray",
            &format!("ts6tray: {UPDATED}"),
            "#progress ts6tray 2/2",
            "3 updated, 1 skipped, 0 failed",
        ],
    );
    r.start();
    let seen = click_watch(&mut r, Action::Update { name: None }, |s| (s.run_all_title(), s.run_counts()));
    // (before the first `#progress` no count is shown: the header names the program)
    let counts: Vec<(usize, usize)> = seen.iter().map(|s| s.1).skip_while(|c| *c != (1, 1)).collect();
    let mut sorted = counts.clone();
    sorted.sort();
    assert_eq!(counts, sorted, "the count never goes backwards: {counts:?}");
    assert_eq!(counts.first(), Some(&(1, 1)));
    assert_eq!(counts.last(), Some(&(3, 3)), "1 (ritz-get) + 2");
    assert!(counts.contains(&(2, 3)), "{counts:?}");
    let heads: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
    assert!(!heads.contains(&"Updating 2 of 1\u{2026}"), "{heads:?}");
    assert_eq!(r.app.state.progress["ts6tray"].label(), "Updated");
    assert_eq!(r.app.state.progress["ritz-get-gui"].label(), "Updated");
}

/// The same run against a ritz-get that prints no markers: the old inference (running = first
/// unfinished, header counts the finished cards).
#[test]
fn update_all_without_markers_is_the_old_behaviour() {
    let env = "FAKE_RITZ_GET_DELAY=0.03 FAKE_RITZ_GET_NO_PROGRESS=1 FAKE_RITZ_GET_UPDATE_ALL_FILE={DIR}/update-all.txt";
    let mut r = Rig::new_env("ok", "full.porcelain", None, env);
    // the markers are in the file; the fake drops them under FAKE_RITZ_GET_NO_PROGRESS
    update_all_file(
        &r,
        &[
            "ritz-get-gui: up to date",
            "bar: held (ritz-get unhold bar)",
            "#start ts6tray",
            &format!("ts6tray: {UPDATED}"),
            "#progress ts6tray 1/1",
            "1 updated, 1 skipped, 0 failed",
        ],
    );
    r.start();
    let seen = click_watch(&mut r, Action::Update { name: None }, |s| {
        (phase_of(s, "ritz-get-gui"), phase_of(s, "ts6tray"), s.run_counts())
    });
    let seen: Vec<_> = seen.into_iter().filter(|(a, _, _)| a.is_some()).collect();
    let up = Progress::Result(Outcome::Updated { from: "2026-10-20 a596443".into(), to: "2026-10-22 b12c3d4".into() });
    assert_eq!(
        seen,
        [
            (Some(Progress::Running), Some(Progress::Queued), (0, 2)),
            (Some(Progress::Result(Outcome::UpToDate)), Some(Progress::Running), (1, 2)),
            (Some(Progress::Result(Outcome::UpToDate)), Some(up), (2, 2)),
        ]
    );
    let s = &r.app.state;
    assert!(!s.marks.started && s.marks.legacy);
    assert_eq!(s.run_position(), (2, 2));
}

/// The failed scenario still gets its `#progress` (after the FAILED line); the card and the log are
/// as without markers.
#[test]
fn failure_still_gets_progress() {
    let mut r = Rig::new_env("failed", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.02");
    r.start();
    r.click(Action::Install { name: "ritz".into(), branch: None });
    let s = &r.app.state;
    assert_eq!(s.marks, Marks { started: true, legacy: false, current: Some("ritz".into()), base: 0, seg: (1, 1) });
    assert_eq!(s.progress["ritz"].phase, Progress::Failed);
    assert!(s.log.iter().all(|l| !l.text.starts_with('#')));
    assert!(s.log.iter().any(|l| l.text.contains("FAILED (install.sh exit 1)")));
}

/// A successful single install: `#start` / `#progress` change nothing visible; M = 1.
#[test]
fn single_install_counts_one_of_one() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_DELAY=0.01");
    r.start();
    r.click(Action::Install { name: "ritz".into(), branch: None });
    assert_eq!(r.app.state.marks.seg, (1, 1));
    assert_eq!(r.app.state.progress["ritz"].phase, Progress::Done);
    assert!(r.app.state.log.iter().all(|l| !l.text.starts_with('#')));
}

// ---- delete program data (`list --data`, `delete-data NAME --yes`) -----------------------------------

fn data_paths(r: &Rig, name: &str) -> Vec<String> {
    r.find(name).data.iter().map(|d| d.path.clone()).collect()
}

fn delete_data(name: &str) -> Action {
    Action::DeleteData { name: name.into() }
}

/// An old ritz-get answers `list --data` with exit 2: the app asks the same list again without it
/// (at the front of the queue), shows no error, and every later list goes without `--data`.
#[test]
fn old_ritz_get_without_data_flag_falls_back_silently() {
    let mut r = Rig::new_env("ok", "full.porcelain", None, "FAKE_RITZ_GET_NO_DATA_FLAG=1");
    r.start();
    let calls = r.calls();
    // The offline list fails with exit 2 and is retried at once without `--data`; the online list
    // behind it was already rewritten to go without it.
    assert_eq!(
        argvs(&calls),
        ["dir", "list --offline --porcelain --data", "list --offline --porcelain", "list --porcelain", "shortcuts"]
    );
    assert_eq!(calls.iter().map(|c| c.exit).collect::<Vec<_>>(), [0, 2, 0, 0, 0]);
    assert_call_invariants(&calls);
    let s = &r.app.state;
    assert!(r.app.no_data_flag);
    // No error line. (The old CLI's stderr line is echoed as plain Info text: that is all the log gets.)
    assert!(!s.log.iter().any(|l| l.kind == LogKind::Err), "no error is surfaced: {:?}", s.log);
    assert!(s.log.iter().all(|l| l.kind == LogKind::Info && l.text == "ritz-get: unknown option: --data"), "{:?}", s.log);
    assert!(s.listing.entries().count() >= 17 && s.checked_at.is_some() && s.actions_enabled());
    assert!(!s.ritz_get_missing && s.refused.is_none());
    assert!(s.listing.entries().all(|e| e.data.is_empty()), "no data without the flag");

    // Later lists (a mutation's refresh, Check for updates) stay without `--data`.
    r.click(Action::Hold { name: "ccorral".into() });
    r.click(Action::CheckForUpdates);
    let calls = r.calls();
    assert_eq!(argvs(&calls)[5..], ["hold ccorral", "list --porcelain", "list --porcelain"]);
    assert!(!calls.iter().skip(2).any(|c| c.argv.contains("--data")));
    assert_call_invariants(&calls);
    assert!(!r.app.state.log.iter().any(|l| l.kind == LogKind::Err), "{:?}", r.app.state.log);
}

/// ritz has two data folders after startup; deleting them runs `delete-data ritz --yes` (never
/// sudo), the dialog ends Done, and the refreshed listing has no data left for ritz.
#[test]
fn delete_data_succeeds_and_the_refresh_drops_it() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    assert_eq!(data_paths(&r, "ritz"), ["~/.config/ritz", "~/.local/share/ritz"]);
    assert!(!r.find("ritz").is_installed());

    r.click(delete_data("ritz"));
    let s = &r.app.state;
    assert_eq!(s.modal, Some(Modal::Run));
    assert_eq!(s.run.as_ref().map(|x| x.verb), Some(Verb::DeleteData));
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
    assert!(s.log.iter().any(|l| l.kind == LogKind::Ok && l.text == "ritz: data deleted"), "{:?}", s.log);
    assert!(!s.log.iter().any(|l| l.kind == LogKind::Err), "{:?}", s.log);
    assert!(s.actions_enabled());
    assert!(r.find("ritz").data.is_empty(), "the refreshed listing has no data left");
    assert_eq!(r.find("ritz").data_label(), None);
    assert_eq!(data_paths(&r, "SteamAlt"), ["~/.config/SteamAlt"], "other programs keep theirs");

    let calls = r.calls();
    assert_eq!(argvs(&calls)[STARTUP.len()..], ["delete-data ritz --yes", "list --porcelain --data"]);
    let c = calls.iter().find(|c| c.argv == "delete-data ritz --yes").unwrap();
    assert_eq!((c.exit, c.sudo.as_str()), (0, "-"));
    assert_call_invariants(&calls);
}

/// An installed program keeps its data: exit 1, "uninstall first", and the listing is unchanged.
#[test]
fn delete_data_of_an_installed_program_is_refused() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    assert!(r.find("SteamAlt").is_installed());
    let before = data_paths(&r, "SteamAlt");
    assert_eq!(before, ["~/.config/SteamAlt"]);
    r.click(delete_data("SteamAlt"));
    assert_eq!(phase(&r), Some(RunPhase::Failed { reason: "uninstall first: ritz-get remove SteamAlt".into() }));
    assert_eq!(r.app.state.modal, Some(Modal::Run));
    assert_eq!(data_paths(&r, "SteamAlt"), before, "its data is unchanged");
    let calls = r.calls();
    let c = calls.iter().find(|c| c.argv.starts_with("delete-data")).unwrap();
    assert_eq!((c.argv.as_str(), c.exit, c.sudo.as_str()), ("delete-data SteamAlt --yes", 1, "-"));
    assert_call_invariants(&calls);
    assert!(r.app.state.actions_enabled());
}

/// Exit 5: the Busy phase and no refresh.
#[test]
fn delete_data_busy_shows_busy_and_refreshes_nothing() {
    let mut r = Rig::new("busy", "full.porcelain");
    r.start();
    let before = r.calls().len();
    r.click(delete_data("ritz"));
    assert_eq!(phase(&r), Some(RunPhase::Busy));
    let calls = r.calls();
    assert_eq!(calls.len(), before + 1, "busy refreshes nothing: {:?}", argvs(&calls));
    let c = calls.last().unwrap();
    assert_eq!((c.argv.as_str(), c.exit, c.sudo.as_str()), ("delete-data ritz --yes", 5, "-"));
    assert_eq!(data_paths(&r, "ritz").len(), 2, "nothing was deleted");
    assert!(r.app.state.actions_enabled() && !r.app.state.busy);
    assert_call_invariants(&calls);
}

/// SCENARIO=failed (exit 1 for every changing command): the dialog ends Failed.
#[test]
fn delete_data_failed_scenario_ends_failed() {
    let mut r = Rig::new("failed", "full.porcelain");
    r.start();
    r.click(delete_data("ritz"));
    assert!(matches!(phase(&r), Some(RunPhase::Failed { .. })), "{:?}", phase(&r));
    assert_eq!(r.calls().iter().find(|c| c.argv == "delete-data ritz --yes").map(|c| c.exit), Some(1));
    assert_call_invariants(&r.calls());
}

/// Dispatched twice (the action is only offered while data exists, but a stale click can happen):
/// the second run is "ritz: no data left", exit 0, and ends Done.
#[test]
fn delete_data_twice_says_no_data_left() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    r.click(delete_data("ritz"));
    r.click(Action::CloseModal);
    r.click(delete_data("ritz"));
    assert_eq!(phase(&r), Some(RunPhase::Done { menu: false }));
    let s = &r.app.state;
    assert!(s.log.iter().any(|l| l.kind == LogKind::Ok && l.text == "ritz: no data left"), "{:?}", s.log);
    assert!(!s.log.iter().any(|l| l.kind == LogKind::Err), "{:?}", s.log);
    assert!(r.find("ritz").data.is_empty());
    let calls = r.calls();
    assert_eq!(
        argvs(&calls)[STARTUP.len()..],
        ["delete-data ritz --yes", "list --porcelain --data", "delete-data ritz --yes", "list --porcelain --data"]
    );
    assert!(calls.iter().all(|c| c.exit == 0));
    assert_call_invariants(&calls);
}

// ---- branch picks are remembered, not applied ----------------------------------------------

fn pick(name: &str, branch: &str) -> Action {
    Action::PickBranch { name: name.into(), branch: branch.into() }
}

/// The calls after the startup ones, `list` refreshes left out.
fn mutations(r: &Rig) -> Vec<String> {
    r.calls().into_iter().skip(STARTUP.len()).map(|c| c.argv).filter(|a| !a.starts_with("list")).collect()
}

/// Picking a branch of a not-installed program runs nothing; Install then installs that branch,
/// and the pick is gone afterwards.
#[test]
fn pick_on_a_not_installed_program_runs_nothing_and_install_uses_it() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    let before = r.calls().len();
    r.click(pick("ForzaTelemetryV3Launcher", "dev"));
    assert_eq!(r.calls().len(), before, "a pick makes no call");
    let e = r.find("ForzaTelemetryV3Launcher");
    assert_eq!(e.pending_branch.as_deref(), Some("dev"));
    assert!(!e.has_update(), "not installed: no update pill");
    assert_eq!(e.install_action(), Action::Install { name: e.name.clone(), branch: Some("dev".into()) });
    // The side panel's Install button carries it.
    let offered = side::offered_actions(&r.app.state, &e);
    assert!(offered.contains(&e.install_action()), "{offered:?}");

    r.click(e.install_action());
    assert_eq!(mutations(&r), ["install ForzaTelemetryV3Launcher --branch dev"]);
    assert!(r.app.state.pending.is_empty(), "applied: forgotten");
    assert_eq!(r.find("ForzaTelemetryV3Launcher").pending_branch, None);
}

/// A failed install keeps the pick for a retry.
#[test]
fn failed_install_keeps_the_pick() {
    let mut r = Rig::new("failed", "full.porcelain");
    r.start();
    r.click(pick("ForzaTelemetryV3Launcher", "dev"));
    r.click(r.find("ForzaTelemetryV3Launcher").install_action());
    assert_eq!(r.app.state.pending.get("ForzaTelemetryV3Launcher").map(String::as_str), Some("dev"));
}

/// Picking a branch of an installed program runs nothing, the entry then counts as an update (pill,
/// filter, Update all count), and Update runs `switch NAME BRANCH`; picking the installed branch
/// again forgets the pick.
#[test]
fn pick_on_an_installed_program_is_an_update_that_switches() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    let (updates, all) = (r.app.state.counts().updates, r.app.state.update_all_count());
    let before = r.calls().len();
    assert!(!r.find("ccorral").has_update());

    r.click(pick("ccorral", "dev"));
    assert_eq!(r.calls().len(), before, "a pick makes no call");
    let e = r.find("ccorral");
    assert!(e.has_update() && e.updatable());
    assert_eq!(r.app.state.counts().updates, updates + 1);
    assert_eq!(r.app.state.update_all_count(), all + 1);
    let pills: Vec<String> = crate::ui::pills::pills(&r.app.state, &e).into_iter().map(|p| p.text).collect();
    assert_eq!(pills[0], "Update available");
    assert_eq!(e.update_action(), Action::Switch { name: "ccorral".into(), branch: "dev".into() });
    assert!(side::offered_actions(&r.app.state, &e).contains(&e.update_action()));

    // Back to the installed branch: no longer an update.
    r.click(pick("ccorral", "main"));
    assert!(!r.find("ccorral").has_update());
    assert!(r.app.state.pending.is_empty());
    assert_eq!(r.calls().len(), before);

    r.click(pick("ccorral", "dev"));
    r.click(r.find("ccorral").update_action());
    assert_eq!(mutations(&r), ["switch ccorral dev"]);
    assert!(r.app.state.pending.is_empty(), "applied: forgotten");
    assert!(!r.find("ccorral").has_update());
}

/// A failed switch keeps the pick (and so the update) for a retry.
#[test]
fn failed_switch_keeps_the_pick() {
    let mut r = Rig::new("failed", "full.porcelain");
    r.start();
    r.click(pick("ccorral", "dev"));
    r.click(r.find("ccorral").update_action());
    assert!(r.find("ccorral").has_update());
    assert_eq!(r.app.state.pending.get("ccorral").map(String::as_str), Some("dev"));
}

/// Update all: a `switch` per picked branch first, then the normal `update --yes` for the rest.
#[test]
fn update_all_switches_the_picked_branches_then_updates_the_rest() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    r.click(pick("ccorral", "dev"));
    r.click(pick("ts6tray", "legacy")); // behind AND picked: the switch covers it
    assert_eq!(r.app.state.update_all_count(), 3, "ritz-get-gui, ts6tray, ccorral");
    r.click(Action::Update { name: None });
    assert_eq!(mutations(&r), ["switch ccorral dev", "switch ts6tray legacy", "update --yes"]);
    assert!(r.app.state.pending.is_empty());
    assert_call_invariants(&r.calls());
}

/// Only picks: Update all runs the switches and no plain `update`.
#[test]
fn update_all_with_only_picks_runs_no_plain_update() {
    let mut r = Rig::new("ok", "full.porcelain");
    r.start();
    let fixture = std::fs::read_to_string(&r.listing).unwrap();
    // Nothing behind: every "behind" becomes "current" in the served listing.
    std::fs::write(&r.listing, fixture.replace("\tbehind\t", "\tcurrent\t").replace("\t3\t", "\t0\t")).unwrap();
    r.click(Action::CheckForUpdates);
    r.click(pick("ccorral", "dev"));
    assert_eq!(r.app.state.update_all_count(), 1);
    r.click(Action::Update { name: None });
    assert_eq!(mutations(&r), ["switch ccorral dev"]);
}
