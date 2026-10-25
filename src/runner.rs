//! Command runner (**I3**): runs one `ritz-get` command at a time on a thread and streams its
//! output to the UI thread.
//!
//! > I3: Command runner: one at a time, streaming thread, `request_repaint`. Env per PORCELAIN.md
//! > (`RITZ_GET_GUI=1` always; `RITZ_GET_SUDO=pkexec` on install/remove/switch/reset only; `--yes`
//! > on update/reset; stdin `/dev/null`). Exit 0-5 mapping; captures `missing:`, `deps:`,
//! > `needs root:` and the per-program `NAME: ...` lines; `branches` parsing; `RITZ_GET` override.
//!
//! Contract with the rest of the app (this file's public items are the interface; extend, don't
//! rename):
//! - argv comes from [`Command::args`] (already includes `--yes`, `--porcelain`, ...), the sudo
//!   env flag from [`Command::uses_sudo`]; the binary is `$RITZ_GET` or `ritz-get` on `PATH`.
//! - For [`Command::is_query`] commands no [`RunnerEvent::Line`] is emitted for stdout (it is only
//!   returned in [`Captured::stdout`]); stderr lines are still streamed.
//! - If the binary cannot be spawned: one stderr `Line` with the OS error, then
//!   `Finished { exit: ExitKind::SpawnFailed, .. }`.
//! - Every thread-side line triggers `ctx.request_repaint()`.
//! - Exit codes map via [`ExitKind::from_code`]; for multi-program runs ritz-get itself already
//!   reports the highest-priority code (3 over 4 over 1).

use std::io::{BufRead, BufReader, Read};
use std::process::{Command as Process, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::model::{Command, ExitKind};

/// Which pipe a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// What happened to one program in an install/update/switch/reset run (the `NAME: ...` lines,
/// PORCELAIN.md "Lines worth parsing").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `NAME: up to date`
    UpToDate,
    /// `NAME: updated A -> B`
    Updated { from: String, to: String },
    /// `NAME: held (ritz-get unhold NAME)`
    Held,
    /// `NAME: SKIPPED (local changes; ritz-get reset NAME to discard them)`
    SkippedLocalChanges,
    /// `NAME: SKIPPED (needs a newer ritz-get)`
    SkippedNeedsNewer,
    /// `NAME: FAILED (reason)`; reasons include `needs: a, b` (exit 3), `timed out after 60 min`,
    /// `fetch failed`, `install.sh exit N`.
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramResult {
    pub name: String,
    pub outcome: Outcome,
}

/// `update` ends with `N updated, N skipped, N failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RunSummary {
    pub updated: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// Everything the app needs from a finished command beyond the exit code (D58).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Captured {
    /// Complete stdout (always filled; this is where `list --porcelain` / `branches` / `dir` text is).
    pub stdout: String,
    /// Raw `missing: TOOL (try: pacman:PKG dnf:PKG apt:PKG)` lines, in order, as printed.
    pub missing: Vec<String>,
    /// The merged install command after `deps: ` (e.g. `sudo pacman -S --needed a b`), last one wins.
    /// `None` on an unknown distro: show `missing` instead (D58).
    pub deps: Option<String>,
    /// Raw `needs root: ...` lines.
    pub needs_root: Vec<String>,
    /// Per-program result lines, in order.
    pub results: Vec<ProgramResult>,
    /// The `N updated, N skipped, N failed` line, if present.
    pub summary: Option<RunSummary>,
}

/// A machine line of ritz-get's progress stream (PORCELAIN.md "Progress lines"; stdout, `RITZ_GET_GUI=1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Marker {
    /// `#start NAME`: NAME's apply step begins.
    Start { name: String },
    /// `#progress NAME N/M`: after NAME's result line, N of M programs of this run are done.
    Progress { name: String, n: usize, m: usize },
}

/// Parse a `#start NAME` / `#progress NAME N/M` line. Other lines (also other `#...` ones) are `None`.
pub fn parse_marker(line: &str) -> Option<Marker> {
    let line = line.trim_end();
    if let Some(name) = line.strip_prefix("#start ") {
        let name = name.trim();
        return (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| Marker::Start { name: name.to_owned() });
    }
    let rest = line.strip_prefix("#progress ")?;
    let (name, frac) = rest.trim().rsplit_once(' ')?;
    let name = name.trim();
    let (n, m) = frac.split_once('/')?;
    let (n, m) = (n.parse::<usize>().ok()?, m.parse::<usize>().ok()?);
    (!name.is_empty() && m > 0).then(|| Marker::Progress { name: name.to_owned(), n, m })
}

/// A stdout machine line (`#start ...`, `#progress ...`, any future `#word ...`): not for people, so
/// the log view and Copy log leave it out. A `# comment` (hash, space) from a build script is text.
pub fn is_machine_line(line: &str) -> bool {
    line.strip_prefix('#').and_then(|r| r.chars().next()).is_some_and(|c| !c.is_whitespace())
}

impl Captured {
    /// Feed one output line (either stream) into the capture: stdout lines are appended to
    /// [`Captured::stdout`]; every line is checked against the PORCELAIN.md "Lines worth parsing".
    /// `missing:` and `needs root:` lines are kept once per distinct text (ritz-get repeats the
    /// `needs root:` lines at the end of the run).
    pub fn absorb(&mut self, line: &str, stream: Stream) {
        if stream == Stream::Stdout {
            self.stdout.push_str(line);
            self.stdout.push('\n');
        }
        let line = line.trim_end();
        if line.starts_with("missing: ") {
            push_unique(&mut self.missing, line);
        } else if let Some(rest) = line.strip_prefix("deps: ") {
            self.deps = Some(rest.trim().to_owned());
        } else if line.starts_with("needs root: ") {
            push_unique(&mut self.needs_root, line);
        } else if let Some(summary) = parse_summary(line) {
            self.summary = Some(summary);
        } else if let Some(result) = parse_result(line) {
            self.results.push(result);
        }
    }
}

fn push_unique(v: &mut Vec<String>, line: &str) {
    if !v.iter().any(|x| x == line) {
        v.push(line.to_owned());
    }
}

/// `N updated, N skipped, N failed`
fn parse_summary(line: &str) -> Option<RunSummary> {
    let mut parts = line.split(", ");
    let mut next = |word: &str| -> Option<u32> {
        parts.next()?.strip_suffix(word)?.strip_suffix(' ')?.parse().ok()
    };
    let updated = next("updated")?;
    let skipped = next("skipped")?;
    let failed = next("failed")?;
    parts.next().is_none().then_some(RunSummary { updated, skipped, failed })
}

/// `NAME: up to date | updated A -> B | held (...) | SKIPPED (...) | FAILED (reason)`.
/// Anything else (`NAME: shortcut created (...)`, `FAILED — your installed version ...`) is not a result.
fn parse_result(line: &str) -> Option<ProgramResult> {
    let (name, rest) = line.split_once(": ")?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let outcome = if rest == "up to date" {
        Outcome::UpToDate
    } else if let Some(versions) = rest.strip_prefix("updated ") {
        let (from, to) = versions.split_once(" -> ")?;
        Outcome::Updated { from: from.trim().to_owned(), to: to.trim().to_owned() }
    } else if rest == "held" || rest.starts_with("held (") {
        Outcome::Held
    } else if rest.starts_with("SKIPPED (local changes") {
        Outcome::SkippedLocalChanges
    } else if rest.starts_with("SKIPPED (needs a newer ritz-get") {
        Outcome::SkippedNeedsNewer
    } else if let Some(reason) = rest.strip_prefix("FAILED (") {
        Outcome::Failed { reason: reason.strip_suffix(')').unwrap_or(reason).to_owned() }
    } else {
        return None;
    };
    Some(ProgramResult { name: name.to_owned(), outcome })
}

/// Events the UI thread drains with [`Runner::poll`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunnerEvent {
    Line { text: String, stream: Stream },
    Finished { command: Command, exit: ExitKind, captured: Captured },
}

/// Messages from the pipe readers / the waiter to the coordinator thread.
enum Msg {
    Line(String, Stream),
    Exited(Option<i32>),
}

/// How long to keep draining the pipes after the child exited (a detached grandchild, e.g.
/// `xdg-open`, may hold them open forever).
const DRAIN_GRACE: Duration = Duration::from_millis(300);

/// One command at a time.
#[derive(Default)]
pub struct Runner {
    running: Option<Command>,
    rx: Option<Receiver<RunnerEvent>>,
}

impl Runner {
    pub fn new() -> Runner {
        Runner::default()
    }

    /// The binary to run: `$RITZ_GET` if set and non-empty, else `ritz-get` (PATH lookup).
    pub fn binary() -> String {
        std::env::var("RITZ_GET").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "ritz-get".into())
    }

    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    #[cfg(test)]
    pub fn running(&self) -> Option<&Command> {
        self.running.as_ref()
    }

    /// Start `cmd` on a thread. Returns `false` (and does nothing) if a command is already running.
    pub fn start(&mut self, cmd: Command, ctx: egui::Context) -> bool {
        self.start_with(Self::binary(), cmd, ctx)
    }

    pub(crate) fn start_with(&mut self, bin: String, cmd: Command, ctx: egui::Context) -> bool {
        if self.running.is_some() {
            return false;
        }
        let (tx, rx) = mpsc::channel();
        self.running = Some(cmd.clone());
        self.rx = Some(rx);
        std::thread::spawn(move || run_command(&bin, cmd, ctx, tx));
        true
    }

    /// Drain the events produced since the last call (non-blocking). After a `Finished` the runner
    /// is idle again.
    pub fn poll(&mut self) -> Vec<RunnerEvent> {
        let mut out = Vec::new();
        let Some(rx) = &self.rx else { return out };
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    let done = matches!(ev, RunnerEvent::Finished { .. });
                    out.push(ev);
                    if done {
                        self.rx = None;
                        self.running = None;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The worker died without reporting (panic): don't stay busy forever.
                    if let Some(command) = self.running.take() {
                        out.push(RunnerEvent::Finished {
                            command,
                            exit: ExitKind::Other(-1),
                            captured: Captured::default(),
                        });
                    }
                    self.rx = None;
                    break;
                }
            }
        }
        out
    }
}

/// Worker thread body: spawn, stream, wait, report.
fn run_command(bin: &str, cmd: Command, ctx: egui::Context, tx: Sender<RunnerEvent>) {
    let emit = |ev: RunnerEvent| {
        let _ = tx.send(ev);
        ctx.request_repaint();
    };
    let mut captured = Captured::default();

    let mut proc = Process::new(bin);
    proc.args(cmd.args())
        .env("RITZ_GET_GUI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cmd.uses_sudo() {
        proc.env("RITZ_GET_SUDO", "pkexec");
    } else {
        // Never inherit it from our own environment.
        proc.env_remove("RITZ_GET_SUDO");
    }
    let mut child = match proc.spawn() {
        Ok(c) => c,
        Err(e) => {
            let text = format!("{bin}: {e}");
            captured.absorb(&text, Stream::Stderr);
            emit(RunnerEvent::Line { text, stream: Stream::Stderr });
            emit(RunnerEvent::Finished { command: cmd, exit: ExitKind::SpawnFailed, captured });
            return;
        }
    };

    let (mtx, mrx) = mpsc::channel::<Msg>();
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, Stream::Stdout, mtx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, Stream::Stderr, mtx.clone());
    }
    {
        let mtx = mtx.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code());
            let _ = mtx.send(Msg::Exited(code));
        });
    }
    drop(mtx);

    let quiet_stdout = cmd.is_query();
    let mut code = None;
    let mut exited = false;
    loop {
        let msg = if exited { mrx.recv_timeout(DRAIN_GRACE).ok() } else { mrx.recv().ok() };
        match msg {
            None => break,
            Some(Msg::Line(text, stream)) => {
                captured.absorb(&text, stream);
                if stream == Stream::Stderr || !quiet_stdout {
                    emit(RunnerEvent::Line { text, stream });
                }
            }
            Some(Msg::Exited(c)) => {
                code = c;
                exited = true;
            }
        }
    }
    emit(RunnerEvent::Finished { command: cmd, exit: ExitKind::from_code(code), captured });
}

fn spawn_reader<R: Read + Send + 'static>(pipe: R, stream: Stream, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n' | b'\r')) {
                        buf.pop();
                    }
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if tx.send(Msg::Line(text, stream)).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// Parse `ritz-get branches NAME` output: one line per remote branch, `* ` marks the current one,
/// two spaces otherwise. Returns `(branch, is_current)` in order.
pub fn branches(output: &str) -> Vec<(String, bool)> {
    output
        .lines()
        .filter_map(|l| {
            let (cur, name) = match l.strip_prefix("* ") {
                Some(rest) => (true, rest),
                None => (false, l),
            };
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), cur))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::sync::{Once, OnceLock};
    use std::time::Instant;

    // ---- absorb / branches (pure) -------------------------------------------------------------

    fn absorb_all(lines: &[&str]) -> Captured {
        let mut c = Captured::default();
        for l in lines {
            c.absorb(l, Stream::Stdout);
        }
        c
    }

    fn res(name: &str, outcome: Outcome) -> ProgramResult {
        ProgramResult { name: name.into(), outcome }
    }

    #[test]
    fn absorb_missing_keeps_raw_lines() {
        let mut c = Captured::default();
        let a = "missing: cmake (try: pacman:cmake dnf:cmake apt:cmake)";
        let b = "missing: pkg-config (try: pacman:pkgconf dnf:pkgconf-pkg-config)";
        c.absorb(a, Stream::Stderr);
        c.absorb(b, Stream::Stderr);
        c.absorb(a, Stream::Stderr); // echoed twice -> kept once
        assert_eq!(c.missing, vec![a, b]);
        assert!(c.stdout.is_empty(), "stderr lines do not land in stdout");
        assert_eq!(c.deps, None);
    }

    #[test]
    fn absorb_deps_last_wins() {
        let c = absorb_all(&["deps: sudo pacman -S --needed a b", "deps: sudo dnf install a b c"]);
        assert_eq!(c.deps.as_deref(), Some("sudo dnf install a b c"));
        let c = absorb_all(&["deps: sudo apt install x"]);
        assert_eq!(c.deps.as_deref(), Some("sudo apt install x"));
    }

    #[test]
    fn absorb_needs_root() {
        let l = "needs root: foo wants /etc access; run `ritz-get install foo` in a terminal";
        let c = absorb_all(&[l, "other", l]);
        assert_eq!(c.needs_root, vec![l]);
    }

    #[test]
    fn absorb_every_result_form() {
        let c = absorb_all(&[
            "a: up to date",
            "geofinder: updated 2026-10-01 abc1234 -> 2026-10-20 def5678",
            "b: updated abc1234 -> def5678",
            "bar: held (ritz-get unhold bar)",
            "baz: SKIPPED (local changes; ritz-get reset baz to discard them)",
            "old: SKIPPED (needs a newer ritz-get)",
            "c: FAILED (fetch failed)",
            "d: FAILED (needs: cmake, pkg-config)",
            "e: FAILED (install.sh exit 7)",
            "f: FAILED (timed out after 60 min)",
        ]);
        assert_eq!(
            c.results,
            vec![
                res("a", Outcome::UpToDate),
                res(
                    "geofinder",
                    Outcome::Updated { from: "2026-10-01 abc1234".into(), to: "2026-10-20 def5678".into() }
                ),
                res("b", Outcome::Updated { from: "abc1234".into(), to: "def5678".into() }),
                res("bar", Outcome::Held),
                res("baz", Outcome::SkippedLocalChanges),
                res("old", Outcome::SkippedNeedsNewer),
                res("c", Outcome::Failed { reason: "fetch failed".into() }),
                res("d", Outcome::Failed { reason: "needs: cmake, pkg-config".into() }),
                res("e", Outcome::Failed { reason: "install.sh exit 7".into() }),
                res("f", Outcome::Failed { reason: "timed out after 60 min".into() }),
            ]
        );
    }

    #[test]
    fn absorb_ignores_non_result_lines() {
        let c = absorb_all(&[
            "FAILED — your installed version is unchanged and still works",
            "foo: shortcut created (/home/u/.local/share/applications/foo.desktop)",
            "foo: needs repair: ritz-get install foo",
            "Additional steps required:",
            "  run foo --setup",
            "Building foo ...",
            "foo: updated garbage",
            "",
        ]);
        assert!(c.results.is_empty(), "{:?}", c.results);
        assert!(c.summary.is_none());
        assert!(c.missing.is_empty() && c.needs_root.is_empty() && c.deps.is_none());
        // but it is all still in stdout (for the log)
        assert!(c.stdout.starts_with("FAILED — your installed version"));
    }

    #[test]
    fn absorb_summary() {
        let c = absorb_all(&["2 updated, 1 skipped, 0 failed"]);
        assert_eq!(c.summary, Some(RunSummary { updated: 2, skipped: 1, failed: 0 }));
        for bad in [
            "2 updated, 1 skipped",
            "x updated, 1 skipped, 0 failed",
            "2 updated, 1 skipped, 0 failed, 3 more",
            "2026-10-24T03:00:01 2 updated, 1 skipped, 0 failed",
        ] {
            assert_eq!(absorb_all(&[bad]).summary, None, "{bad}");
        }
    }

    #[test]
    fn absorb_stdout_accumulates_with_newlines() {
        let c = absorb_all(&["one", "two"]);
        assert_eq!(c.stdout, "one\ntwo\n");
    }

    #[test]
    fn marker_lines_parse() {
        assert_eq!(parse_marker("#start ritz"), Some(Marker::Start { name: n("ritz") }));
        assert_eq!(parse_marker("#start yt-dlp-gui\r"), Some(Marker::Start { name: n("yt-dlp-gui") }));
        assert_eq!(parse_marker("#progress ritz 1/3"), Some(Marker::Progress { name: n("ritz"), n: 1, m: 3 }));
        assert_eq!(parse_marker("#progress a.b_c 12/12"), Some(Marker::Progress { name: n("a.b_c"), n: 12, m: 12 }));
        assert_eq!(parse_marker("#progress ritz 0/2"), Some(Marker::Progress { name: n("ritz"), n: 0, m: 2 }));
        for bad in [
            "", "#start", "#start ", "#start a b", "#progress", "#progress ritz", "#progress ritz 1", "#progress ritz 1/",
            "#progress ritz /3", "#progress ritz x/3", "#progress ritz 1/0", "#progress ritz -1/2", "#progress 1/2",
            "#porcelain 1", "#online 1", "#hidden 3", "#starting x", "# start x", "ritz: #start x", "#progressx a 1/2",
        ] {
            assert_eq!(parse_marker(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn machine_lines_are_hash_without_a_space() {
        for l in ["#start x", "#progress x 1/2", "#porcelain 1", "#future thing"] {
            assert!(is_machine_line(l), "{l}");
        }
        for l in ["", "#", "# a comment", "#\tx", "x #start y", "ritz: updated a -> b"] {
            assert!(!is_machine_line(l), "{l}");
        }
    }

    #[test]
    fn marker_lines_are_not_result_lines() {
        let c = absorb_all(&["#start ritz", "ritz: up to date", "#progress ritz 1/1"]);
        assert_eq!(c.results.len(), 1);
        assert_eq!(c.stdout, "#start ritz\nritz: up to date\n#progress ritz 1/1\n");
    }

    #[test]
    fn branches_parse() {
        assert_eq!(
            branches("* main\n  dev\n  legacy\n"),
            vec![("main".into(), true), ("dev".into(), false), ("legacy".into(), false)]
        );
        assert_eq!(branches("  a\n* b/c-1\n\n"), vec![("a".into(), false), ("b/c-1".into(), true)]);
        assert!(branches("").is_empty());
        assert!(branches("\n  \n").is_empty());
    }

    // ---- process tests (tiny sh scripts, no display needed) ----------------------------------

    struct Scripts {
        env: String,
        exit: String,
        slow: String,
        query: String,
        odd: String,
        detach: String,
        fake_deps: String,
        fake_nodeps: String,
        fake_needs_root: String,
        fake_busy: String,
        fake_failed: String,
    }

    /// All helper scripts are written once, before any test spawns anything, so no fork can
    /// inherit a still-open write fd (ETXTBSY).
    fn scripts() -> &'static Scripts {
        static S: OnceLock<Scripts> = OnceLock::new();
        static ENV: Once = Once::new();
        ENV.call_once(|| {
            // A parent environment that must NOT leak into the children.
            // SAFETY: done once, before any child is spawned; std serialises env access.
            unsafe {
                std::env::set_var("RITZ_GET_SUDO", "leaked-from-parent");
                std::env::set_var("RITZ_GET_GUI", "0");
            }
        });
        S.get_or_init(|| {
            let dir: PathBuf = std::env::temp_dir().join(format!("ritz-get-gui-runner-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let write = |name: &str, body: &str| -> String {
                let p = dir.join(name);
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o755)
                    .open(&p)
                    .unwrap();
                f.write_all(format!("#!/bin/sh\n{body}\n").as_bytes()).unwrap();
                p.to_string_lossy().into_owned()
            };
            let fake = format!("{}/tests/fake-ritz-get", env!("CARGO_MANIFEST_DIR"));
            let wrap = |name: &str, scenario: &str| {
                write(
                    name,
                    &format!("FAKE_RITZ_GET_DELAY=0 FAKE_RITZ_GET_SCENARIO={scenario} exec '{fake}' \"$@\""),
                )
            };
            Scripts {
                env: write(
                    "env.sh",
                    r#"echo "gui=${RITZ_GET_GUI-unset}"
echo "sudo=${RITZ_GET_SUDO-unset}"
echo "argv=$*"
if [ -t 0 ]; then echo tty=1; else echo tty=0; fi
if read -r line; then echo "stdin=data"; else echo "stdin=eof"; fi"#,
                ),
                // `hold N` / `list ...` etc: exit with N taken from the second arg; `kill` -> signal.
                exit: write("exit.sh", r#"if [ "$2" = kill ]; then kill -9 $$; fi; exit "$2""#),
                slow: write("slow.sh", "sleep 1\necho done"),
                query: write("query.sh", "echo out1\necho err1 >&2\necho out2"),
                odd: write("odd.sh", r"printf 'crlf\r\n'; printf 'bad\377byte\n'; printf 'no newline at end'"),
                detach: write("detach.sh", "sleep 5 &\necho hi"),
                fake_deps: wrap("fake-deps.sh", "deps"),
                fake_nodeps: wrap("fake-nodeps.sh", "deps-nodeps"),
                fake_needs_root: wrap("fake-needs-root.sh", "needs-root"),
                fake_busy: wrap("fake-busy.sh", "busy"),
                fake_failed: wrap("fake-failed.sh", "failed"),
            }
        })
    }

    /// Run `cmd` against `bin` to completion; all events, `Finished` last.
    fn run(bin: &str, cmd: Command) -> Vec<RunnerEvent> {
        let mut r = Runner::new();
        assert!(r.start_with(bin.into(), cmd, egui::Context::default()));
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut events = Vec::new();
        loop {
            events.extend(r.poll());
            if matches!(events.last(), Some(RunnerEvent::Finished { .. })) {
                assert!(!r.is_busy(), "idle after Finished");
                return events;
            }
            assert!(Instant::now() < deadline, "timed out; events so far: {events:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn finished(events: &[RunnerEvent]) -> (&Command, ExitKind, &Captured) {
        match events.last() {
            Some(RunnerEvent::Finished { command, exit, captured }) => (command, *exit, captured),
            other => panic!("last event is not Finished: {other:?}"),
        }
    }

    fn lines(events: &[RunnerEvent], want: Stream) -> Vec<&str> {
        events
            .iter()
            .filter_map(|e| match e {
                RunnerEvent::Line { text, stream } if *stream == want => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn n(s: &str) -> String {
        s.to_owned()
    }

    fn all_commands() -> Vec<Command> {
        vec![
            Command::List { offline: true, all: false },
            Command::List { offline: false, all: true },
            Command::Install { name: n("foo"), branch: None },
            Command::Install { name: n("foo"), branch: Some(n("dev")) },
            Command::Update { name: None },
            Command::Update { name: Some(n("foo")) },
            Command::Remove { name: n("foo") },
            Command::Switch { name: n("foo"), branch: n("dev") },
            Command::Reset { name: n("foo") },
            Command::Hold { name: n("foo") },
            Command::Unhold { name: n("foo") },
            Command::Open { name: n("foo") },
            Command::Shortcut { name: n("foo"), remove: true },
            Command::Shortcuts { path: None },
            Command::Shortcuts { path: Some(n("/x")) },
            Command::Dir { path: None },
            Command::Dir { path: Some(n("/x")) },
            Command::Branches { name: n("foo") },
        ]
    }

    #[test]
    fn env_gui_always_sudo_only_when_needed_and_never_inherited() {
        let s = scripts();
        for cmd in all_commands() {
            let ev = run(&s.env, cmd.clone());
            let (_, exit, cap) = finished(&ev);
            assert_eq!(exit, ExitKind::Ok);
            let out = &cap.stdout; // captured for queries and non-queries alike
            assert!(out.contains("gui=1\n"), "{cmd}: RITZ_GET_GUI must be 1 even if the parent has 0:\n{out}");
            if cmd.uses_sudo() {
                assert!(out.contains("sudo=pkexec\n"), "{cmd}:\n{out}");
            } else {
                assert!(out.contains("sudo=unset\n"), "{cmd}: sudo leaked from the parent env:\n{out}");
            }
        }
    }

    #[test]
    fn argv_is_command_args() {
        let s = scripts();
        for cmd in all_commands() {
            let ev = run(&s.env, cmd.clone());
            let (_, _, cap) = finished(&ev);
            let want = format!("argv={}\n", cmd.args().join(" "));
            assert!(cap.stdout.contains(&want), "{cmd}: want {want:?} in\n{}", cap.stdout);
        }
        // the flags the GUI relies on
        let ev = run(&s.env, Command::Update { name: None });
        assert!(finished(&ev).2.stdout.contains("argv=update --yes\n"));
        let ev = run(&s.env, Command::Reset { name: n("x") });
        assert!(finished(&ev).2.stdout.contains("argv=reset x --yes\n"));
        let ev = run(&s.env, Command::List { offline: true, all: false });
        assert!(finished(&ev).2.stdout.contains("argv=list --offline --porcelain\n"));
    }

    #[test]
    fn stdin_is_null_not_a_tty() {
        let ev = run(&scripts().env, Command::Hold { name: n("foo") });
        let out = &finished(&ev).2.stdout;
        assert!(out.contains("tty=0\n"), "{out}");
        assert!(out.contains("stdin=eof\n"), "{out}");
    }

    #[test]
    fn busy_runner_refuses_second_start_and_recovers() {
        let s = scripts();
        let ctx = egui::Context::default();
        let mut r = Runner::new();
        assert!(!r.is_busy() && r.running().is_none());
        let first = Command::Hold { name: n("a") };
        assert!(r.start_with(s.slow.clone(), first.clone(), ctx.clone()));
        assert!(r.is_busy());
        assert_eq!(r.running(), Some(&first));
        assert!(!r.start_with(s.slow.clone(), Command::Hold { name: n("b") }, ctx.clone()));
        assert_eq!(r.running(), Some(&first), "refused start must not replace the running command");
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut events = Vec::new();
        while r.is_busy() {
            events.extend(r.poll());
            assert!(Instant::now() < deadline, "never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        let (cmd, exit, _) = finished(&events);
        assert_eq!((cmd, exit), (&first, ExitKind::Ok), "only the first command ran");
        assert_eq!(lines(&events, Stream::Stdout), vec!["done"]);
        // idle again: a new command is accepted and poll() on an idle runner is empty
        assert!(r.poll().is_empty());
        assert!(r.start_with(s.query.clone(), Command::Branches { name: n("a") }, ctx));
        while r.is_busy() {
            r.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn spawn_failure_reports_spawn_failed() {
        scripts();
        for bin in ["/nonexistent/ritz-get-for-tests", "ritz-get-definitely-not-on-path-xyz"] {
            let ev = run(bin, Command::Update { name: None });
            assert_eq!(ev.len(), 2, "{ev:?}");
            match &ev[0] {
                RunnerEvent::Line { text, stream: Stream::Stderr } => assert!(text.contains(bin), "{text}"),
                other => panic!("{other:?}"),
            }
            assert_eq!(finished(&ev).1, ExitKind::SpawnFailed);
        }
        // a non-executable file is also a spawn failure
        let ev = run("/etc/hostname", Command::Hold { name: n("a") });
        assert_eq!(finished(&ev).1, ExitKind::SpawnFailed);
    }

    #[test]
    fn exit_codes_map() {
        let s = scripts();
        let want = [
            ("0", ExitKind::Ok),
            ("1", ExitKind::Failed),
            ("2", ExitKind::Usage),
            ("3", ExitKind::Deps),
            ("4", ExitKind::NeedsRoot),
            ("5", ExitKind::Busy),
            ("7", ExitKind::Other(7)),
            ("kill", ExitKind::Other(-1)),
        ];
        for (arg, kind) in want {
            let ev = run(&s.exit, Command::Hold { name: arg.into() });
            assert_eq!(finished(&ev).1, kind, "exit {arg}");
        }
    }

    #[test]
    fn query_stdout_is_captured_not_streamed_stderr_is() {
        let s = scripts();
        let ev = run(&s.query, Command::Branches { name: n("a") });
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::Ok);
        assert_eq!(cap.stdout, "out1\nout2\n");
        assert!(lines(&ev, Stream::Stdout).is_empty(), "{ev:?}");
        assert_eq!(lines(&ev, Stream::Stderr), vec!["err1"]);
        // the same script as a non-query streams both
        let ev = run(&s.query, Command::Hold { name: n("a") });
        assert_eq!(lines(&ev, Stream::Stdout), vec!["out1", "out2"]);
        assert_eq!(lines(&ev, Stream::Stderr), vec!["err1"]);
        assert_eq!(finished(&ev).2.stdout, "out1\nout2\n");
        // other query shapes
        for cmd in [
            Command::List { offline: false, all: false },
            Command::Dir { path: None },
            Command::Shortcuts { path: None },
        ] {
            assert!(cmd.is_query());
            let ev = run(&s.query, cmd);
            assert!(lines(&ev, Stream::Stdout).is_empty());
        }
        let ev = run(&s.query, Command::Dir { path: Some(n("/x")) });
        assert_eq!(lines(&ev, Stream::Stdout), vec!["out1", "out2"], "dir PATH is not a query");
    }

    #[test]
    fn odd_output_is_tolerated() {
        let ev = run(&scripts().odd, Command::Hold { name: n("a") });
        let out = lines(&ev, Stream::Stdout);
        assert_eq!(out, vec!["crlf", "bad\u{fffd}byte", "no newline at end"]);
    }

    #[test]
    fn detached_grandchild_holding_the_pipes_does_not_hang_finish() {
        let t = Instant::now();
        let ev = run(&scripts().detach, Command::Open { name: n("site") });
        assert!(t.elapsed() < Duration::from_secs(4), "took {:?}", t.elapsed());
        assert_eq!(finished(&ev).1, ExitKind::Ok);
        assert_eq!(lines(&ev, Stream::Stdout), vec!["hi"]);
    }

    // ---- against the fake CLI ------------------------------------------------------------------

    fn fake() -> String {
        scripts();
        format!("{}/tests/fake-ritz-get", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn fake_update_ok_is_parsed() {
        let ev = run(&fake(), Command::Update { name: None });
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::Ok);
        let names: Vec<_> = cap.results.iter().map(|r| r.name.as_str()).collect();
        // phase 1 (held / skipped / up to date) first, then the apply list
        assert_eq!(names, ["bar", "baz", "qux", "geofinder"]);
        assert_eq!(cap.summary, Some(RunSummary { updated: 1, skipped: 2, failed: 0 }));
        // streamed in order, line by line, with the `#start` / `#progress` pair around the one applied
        let out = lines(&ev, Stream::Stdout);
        assert_eq!(out.len(), 8);
        assert_eq!(parse_marker(out[4]), Some(Marker::Start { name: n("geofinder") }));
        assert_eq!(parse_marker(out[6]), Some(Marker::Progress { name: n("geofinder"), n: 1, m: 1 }));
    }

    #[test]
    fn fake_branches_query() {
        let ev = run(&fake(), Command::Branches { name: n("foo") });
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::Ok);
        assert_eq!(branches(&cap.stdout), vec![(n("main"), true), (n("dev"), false), (n("legacy"), false)]);
        assert!(lines(&ev, Stream::Stdout).is_empty());
    }

    #[test]
    fn fake_scenarios() {
        let s = scripts();
        let install = || Command::Install { name: n("foo"), branch: None };

        let ev = run(&s.fake_deps, install());
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::Deps);
        assert_eq!(cap.missing.len(), 2);
        assert!(cap.missing[0].starts_with("missing: cmake (try: pacman:cmake"));
        assert_eq!(cap.deps.as_deref(), Some("sudo pacman -S --needed cmake pkgconf"));
        assert_eq!(
            cap.results,
            vec![res("foo", Outcome::Failed { reason: "needs: cmake, pkg-config".into() })]
        );
        // the follow-up text still reaches the log
        assert!(lines(&ev, Stream::Stdout).iter().any(|l| l.starts_with("FAILED — your installed version")));
        assert!(lines(&ev, Stream::Stderr).iter().any(|l| l.starts_with("missing: cmake")));

        let ev = run(&s.fake_nodeps, install());
        let (_, exit, cap) = finished(&ev);
        assert_eq!((exit, cap.missing.len(), cap.deps.clone()), (ExitKind::Deps, 2, None));

        let ev = run(&s.fake_needs_root, install());
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::NeedsRoot);
        assert_eq!(cap.needs_root.len(), 1);
        assert!(cap.needs_root[0].starts_with("needs root: "));

        let ev = run(&s.fake_busy, Command::Update { name: None });
        assert_eq!(finished(&ev).1, ExitKind::Busy);
        assert_eq!(lines(&ev, Stream::Stderr), vec!["busy: another ritz-get is running"]);

        let ev = run(&s.fake_failed, Command::Update { name: Some(n("foo")) });
        let (_, exit, cap) = finished(&ev);
        assert_eq!(exit, ExitKind::Failed);
        assert_eq!(cap.results, vec![res("foo", Outcome::Failed { reason: "install.sh exit 1".into() })]);
        assert_eq!(cap.summary, Some(RunSummary { updated: 0, skipped: 0, failed: 1 }));
    }
}
