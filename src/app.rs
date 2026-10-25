//! App state and logic (**I7**).
//!
//! > I7: App state and logic: offline-then-online startup, `ritz-get dir`, filter/search/sort with
//! > counts, Update all (N), sort persisted, session badges from exits 3/4 (NEEDS from
//! > `missing:`/`FAILED (needs: ...)`, SETUP NEEDED from `needs root:`), last-run incl. the
//! > `failed:` names, `#porcelain` refusal, self-update restart flag, failed reset keeps MODIFIED,
//! > `RITZ_GET_GUI_SHOT` hook; unit tests.
//!
//! **Contract with the UI panes (I8-I13):** panes take `&AppState` (read-only) and push
//! [`Action`]s into a `&mut Vec<Action>`; [`App::update`] draws, then dispatches. The *field names
//! and the helper signatures below are frozen*: I7 may add fields/methods and replace every body,
//! but must not rename or retype what is here, or the six UI workers break.
//!
//! # How the pieces fit (I7)
//!
//! * **Startup** ([`App::begin_startup`]) queues, in this order: `dir` (tiny, local; the icons
//!   need the clone folder early), `list --offline --porcelain` (fast first paint), `list
//!   --porcelain` (replaces it, sets `checked_at`), `shortcuts`. The runner runs one command at a
//!   time, so [`App`] keeps a FIFO queue and starts the next one only on `Finished`.
//! * **Dispatch** ([`App::dispatch`]): UI-only actions go through `AppState::apply_ui_action`;
//!   ritz-get actions through `Action::to_command` -> queue; xdg-open / clipboard / restart are
//!   handled in-process (never through the runner).
//! * **Events** ([`App::handle_event`]) is the only place runner output touches the state, so it
//!   is unit-testable without spawning anything.
//! * **UI contract:** disable every action control unless [`AppState::actions_enabled`].
//!   `state.busy` is true while *any* command (also a `list`) runs.
//! * **Screenshot hook (D57):** `RITZ_GET_GUI_SHOT`, applied once the first listing is in (the
//!   `busy` spec once startup is idle). `menu=<id>` sets [`AppState::open_menu`]: the card with
//!   that id must open its `...` popover on the first frame it sees it set; `App::update` clears
//!   the field after that frame, so a pane may simply do
//!   `if state.open_menu.as_deref() == Some(&id) { ui.memory_mut(|m| m.open_popup(popup_id)) }`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::icons::{self, IconFetcher};
use crate::model::*;
use crate::porcelain::{self, PorcelainError};
use crate::runner::{self, Captured, Marker, Outcome, Runner, RunnerEvent, Stream};

/// Tab counts (over the whole listing, `unreachable` entries excluded: D24).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Counts {
    pub all: usize,
    pub installed: usize,
    pub updates: usize,
    pub not_installed: usize,
}

impl Counts {
    pub fn get(&self, f: Filter) -> usize {
        match f {
            Filter::All => self.all,
            Filter::Installed => self.installed,
            Filter::Updates => self.updates,
            Filter::NotInstalled => self.not_installed,
        }
    }
}

/// How long a finished bar (Updated / FAILED / ...) stays on its card before the normal button returns.
pub const PROGRESS_FADE: std::time::Duration = std::time::Duration::from_millis(2000);

/// What the running command does to one card (picks the bar's wording).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Install,
    /// `install NAME` on an installed program.
    Repair,
    Update,
    Switch,
    Reset,
    /// Only the progress dialog (a removal has no card bar).
    Remove,
}

impl Verb {
    /// The bar label while it runs.
    pub fn ing(self) -> &'static str {
        match self {
            Verb::Install => "Installing\u{2026}",
            Verb::Repair => "Repairing\u{2026}",
            Verb::Update => "Updating\u{2026}",
            Verb::Switch => "Switching\u{2026}",
            Verb::Reset => "Resetting\u{2026}",
            Verb::Remove => "Removing\u{2026}",
        }
    }

    /// `ing()` without the ellipsis: "Installing".
    pub fn gerund(self) -> &'static str {
        self.ing().trim_end_matches('\u{2026}')
    }

    /// "Installed", "Updated", ...
    pub fn past(self) -> &'static str {
        match self {
            Verb::Install => "Installed",
            Verb::Repair => "Repaired",
            Verb::Update => "Updated",
            Verb::Switch => "Switched",
            Verb::Reset => "Reset",
            Verb::Remove => "Removed",
        }
    }

    /// "install", "update", ... (for "Couldn't install NAME").
    pub fn base(self) -> &'static str {
        match self {
            Verb::Install => "install",
            Verb::Repair => "repair",
            Verb::Update => "update",
            Verb::Switch => "switch",
            Verb::Reset => "reset",
            Verb::Remove => "remove",
        }
    }
}

/// Where the progress dialog (`Modal::Run`) is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunPhase {
    /// The command runs (or it ended well and the refreshed listing is not in yet).
    Running,
    /// Finished well. `menu`: ask "Add it to the start menu?" (D47: fresh install, column 17 `off`).
    Done { menu: bool },
    /// Exit 1 / other: `reason` is the FAILED reason (or the last red log line).
    Failed { reason: String },
    /// Exit 3: the missing tools; `command` = the merged `deps:` line (None on an unknown distro).
    Deps { missing: Vec<String>, command: Option<String> },
    /// Exit 4 (SETUP NEEDED): the program needs a terminal install.
    Setup,
    /// Exit 5: another ritz-get holds the lock.
    Busy,
}

/// The install / update / repair / switch / reset / remove (or Update all) the progress dialog shows.
/// The rows of Update all are [`AppState::progress`] (in `order`); the log is a slice of [`AppState::log`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub cmd: Command,
    pub verb: Verb,
    /// The program (`None` = Update all).
    pub name: Option<String>,
    /// Update all: the programs in CLI order (a copy of `progress_order` taken at the start; the
    /// bars themselves are `AppState::progress`).
    pub order: Vec<String>,
    pub phase: RunPhase,
    /// The command ended well; waiting for the refreshed listing to decide the start-menu question.
    pub settling: bool,
    /// The refreshed listing asks for the start-menu question (D47).
    pub ask_menu: bool,
    /// A single update's `A -> B`.
    pub change: Option<(String, String)>,
    /// Absolute index (counting lines dropped by the cap) of the run's `$ ritz-get ...` log line.
    log_start: usize,
}

impl Run {
    pub fn is_all(&self) -> bool {
        self.name.is_none()
    }

    pub fn running(&self) -> bool {
        self.phase == RunPhase::Running
    }

    /// Identifies the run (its first log line's absolute index): keys the log view's caches.
    pub fn log_start(&self) -> usize {
        self.log_start
    }
}

/// Commands that open the progress dialog.
pub fn opens_run(c: &Command) -> bool {
    matches!(c, Command::Install { .. } | Command::Update { .. } | Command::Switch { .. } | Command::Reset { .. } | Command::Remove { .. })
}

/// Where one card is in the running command. There is no percentage (the CLI prints none): a card
/// is waiting, running (indeterminate bar), or has its result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Update all: in the set, not reached yet.
    Queued,
    Running,
    /// The command ended well without a per-program result line (install, ...).
    Done,
    /// The program's `NAME: ...` result line (never `Failed`: that is [`Progress::Failed`]).
    Result(Outcome),
    /// `NAME: FAILED (...)`, or the single command exited non-zero.
    Failed,
}

/// How a bar is coloured (shared by the card bar and the side panel row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Animated, ACCENT.
    Busy,
    /// Static, dim.
    Queued,
    /// GOOD (updated / done).
    Good,
    /// Up to date: dim with a check.
    Even,
    /// WARN (skipped).
    Warn,
    /// DANGER (failed).
    Danger,
}

/// What ritz-get's `#start NAME` / `#progress NAME N/M` lines said about the running command
/// (PORCELAIN.md "Progress lines"). Without any of them (an older ritz-get) the app infers the
/// running program from the result lines instead: [`AppState::promote_progress`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// A `#start` has arrived: the markers drive the running card and the counts.
    pub started: bool,
    /// No `#start` came before a result that only an apply step can print (an update, a build
    /// failure): this ritz-get prints no markers, the inference stays.
    pub legacy: bool,
    /// The program of the latest `#start`.
    pub current: Option<String>,
    /// Programs done in the earlier segments of the run. ritz-get re-executes itself after updating
    /// itself and counts the second part on its own (`1/1`, then `1/M`): the parts add up.
    pub base: usize,
    /// The latest `N/M` of the current segment (`(0, 0)` until its first `#progress`).
    pub seg: (usize, usize),
}

impl Marks {
    /// Programs done, over all segments.
    pub fn done(&self) -> usize {
        self.base + self.seg.0
    }

    /// Programs in the run so far known to ritz-get (all segments); `None` until the first `#progress`.
    pub fn total(&self) -> Option<usize> {
        (self.seg.1 > 0).then_some(self.base + self.seg.1)
    }
}

/// One card's bar: its [`Progress`], the verb, and when it became final (the fade clock).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardProgress {
    pub phase: Progress,
    pub verb: Verb,
    /// Set when the command finished; the bar goes [`PROGRESS_FADE`] later (once the refresh is in).
    pub ended: Option<Instant>,
}

impl CardProgress {
    pub fn is_final(&self) -> bool {
        !matches!(self.phase, Progress::Queued | Progress::Running)
    }

    pub fn tone(&self) -> Tone {
        match &self.phase {
            Progress::Queued => Tone::Queued,
            Progress::Running => Tone::Busy,
            Progress::Done => Tone::Good,
            Progress::Failed => Tone::Danger,
            Progress::Result(Outcome::Updated { .. }) => Tone::Good,
            Progress::Result(Outcome::UpToDate) => Tone::Even,
            Progress::Result(_) => Tone::Warn,
        }
    }

    /// The text inside the bar.
    pub fn label(&self) -> &'static str {
        match &self.phase {
            Progress::Queued => "Queued",
            Progress::Running => self.verb.ing(),
            Progress::Done => "Done",
            Progress::Failed => "FAILED",
            Progress::Result(Outcome::Updated { .. }) if self.verb == Verb::Update => "Updated",
            Progress::Result(Outcome::Updated { .. }) => "Done",
            Progress::Result(Outcome::UpToDate) => "Up to date",
            Progress::Result(_) => "Skipped",
        }
    }
}

/// Everything the UI reads. Mutated only by [`App`] (never by a pane).
pub struct AppState {
    /// Latest parsed `list --porcelain` (empty until the first one lands).
    pub listing: Listing,
    /// Programs page or Settings page.
    pub view: View,
    pub filter: Filter,
    pub sort: Sort,
    /// Touch mode: drag-to-scroll in every scroll area (off by default, persisted as `touch=1`).
    pub touch_mode: bool,
    /// Search text (matches display title, name and description, case-insensitive).
    pub search: String,
    /// `Entry::id` (`"section/name"`) of the selected card.
    pub selected: Option<String>,
    /// Names of collapsed sections.
    pub collapsed: HashSet<String>,
    /// The open modal, if any.
    pub modal: Option<Modal>,
    /// The log, oldest first.
    pub log: Vec<LogLine>,
    /// How many of the oldest log lines the cap has dropped so far (keeps [`Run`]'s start valid).
    pub log_dropped: usize,
    /// The progress dialog's content (`Modal::Run`); `Some` while the run is backgrounded too.
    pub run: Option<Run>,
    /// The progress dialog shows its log.
    pub run_log_open: bool,
    /// A command is running: every action control is disabled (D54).
    pub busy: bool,
    /// `busy`, but only read-only queries (`list`, `branches`, `dir`, `shortcuts`) run or wait: the
    /// action controls stay enabled and a mutating command clicked meanwhile queues behind them.
    pub busy_queries_only: bool,
    /// The running command (log header shows it).
    pub running: Option<Command>,
    /// Session badges by entry **name** (D58): NEEDS <tool> / SETUP NEEDED.
    pub badges: HashMap<String, Badge>,
    /// `~/.cache/ritz-get/last-run`, when present.
    pub last_run: Option<LastRun>,
    /// `Some(v)`: the listing's `#porcelain` version is unsupported -> refuse + offer "update ritz-get-gui" (D32).
    pub refused: Option<u32>,
    /// Show the "ritz-get-gui was updated. Restart" bar (D25).
    pub restart: bool,
    /// The `ritz-get` binary could not be spawned: top banner + actions disabled (D58).
    pub ritz_get_missing: bool,
    /// Clone folder from `ritz-get dir` (icons, Open folder, Settings).
    pub dir: Option<PathBuf>,
    /// Shortcuts folder from `ritz-get shortcuts`.
    pub shortcuts_dir: Option<PathBuf>,
    /// Branch lists from `branches NAME`, keyed by entry **name**: `(branch, is_current)`.
    pub branches: HashMap<String, Vec<(String, bool)>>,
    /// A `list` is in flight (status bar "checking...").
    pub loading: bool,
    /// When the last successful online `list` landed ("checked 3 min ago").
    pub checked_at: Option<Instant>,
    /// Screenshot hook `menu=<id>` (D57): the card with this `Entry::id` opens its `...` popover on
    /// the next frame; `App::update` clears it after that frame. `None` in normal use.
    pub open_menu: Option<String>,
    /// Stacked layout (window < 760 px): scroll the body down to the side panel on the next frame
    /// (a card was just selected, or the shot hook asks for it). `App::update` clears it after drawing.
    pub scroll_to_side: bool,
    /// One-shot: scroll the groups pane to the selected card on the next frame (the selection just
    /// changed, or the shot hook asks for it). The card skips it when it is already fully visible,
    /// so a click never recentres the view. `App::update` clears it after drawing. In the stacked
    /// layout the side-panel scroll (registered later in the frame) wins.
    pub scroll_to_card: bool,
    /// Per-card bars of the running (or just finished) install / update / switch / reset, by entry
    /// **name**. Empty when nothing is tracked. See [`AppState::begin_progress`].
    pub progress: HashMap<String, CardProgress>,
    /// Update all: the names in the order the CLI processes them (manifest order); empty for a single command.
    pub progress_order: Vec<String>,
    /// The `#start` / `#progress` lines of the running (or just finished) tracked command.
    pub marks: Marks,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            listing: Listing::default(),
            view: View::Main,
            filter: Filter::All,
            sort: Sort::Default,
            touch_mode: false,
            search: String::new(),
            selected: None,
            collapsed: HashSet::new(),
            modal: None,
            log: Vec::new(),
            log_dropped: 0,
            run: None,
            run_log_open: false,
            busy: false,
            busy_queries_only: false,
            running: None,
            badges: HashMap::new(),
            last_run: None,
            refused: None,
            restart: false,
            ritz_get_missing: false,
            dir: None,
            shortcuts_dir: None,
            branches: HashMap::new(),
            loading: false,
            checked_at: None,
            open_menu: None,
            scroll_to_side: false,
            scroll_to_card: false,
            progress: HashMap::new(),
            progress_order: Vec::new(),
            marks: Marks::default(),
        }
    }
}

impl AppState {
    /// A state with the in-code sample listing and a selection (tests).
    #[cfg(test)]
    pub fn sample() -> AppState {
        let mut s = AppState { listing: crate::sample::listing(), ..AppState::default() };
        s.selected = Some("Helpers/ts6tray".into());
        s
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_deref().and_then(|id| self.listing.find(id))
    }

    pub fn badge(&self, e: &Entry) -> Option<&Badge> {
        self.badges.get(&e.name)
    }

    /// Entries shown in the groups pane: per section (manifest order, `Unlisted` last; empty
    /// sections omitted) the entries passing filter + search, sorted per [`Sort`] (D43).
    /// `unreachable` entries are never shown (D24).
    pub fn visible_sections(&self) -> Vec<(&Section, Vec<&Entry>)> {
        let q = self.search.trim().to_lowercase();
        let matches = |e: &Entry| {
            e.state != State::Unreachable
                && self.filter.matches(e)
                && (q.is_empty()
                    || e.display().to_lowercase().contains(&q)
                    || e.name.to_lowercase().contains(&q)
                    || e.desc.to_lowercase().contains(&q))
        };
        self.listing
            .sections
            .iter()
            .filter_map(|s| {
                let mut v: Vec<&Entry> = s.entries.iter().filter(|e| matches(e)).collect();
                match self.sort {
                    Sort::Default => {}
                    Sort::Name => v.sort_by_key(|e| e.display().to_lowercase()),
                    Sort::UpdatesFirst => v.sort_by_key(|e| !e.has_update()),
                    Sort::InstalledFirst => v.sort_by_key(|e| !e.is_installed()),
                }
                (!v.is_empty()).then_some((s, v))
            })
            .collect()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for e in self.listing.entries().filter(|e| e.state != State::Unreachable) {
            c.all += 1;
            c.installed += Filter::Installed.matches(e) as usize;
            c.updates += Filter::Updates.matches(e) as usize;
            c.not_installed += Filter::NotInstalled.matches(e) as usize;
        }
        c
    }

    /// N in "Update all (N)": behind, not held, not modified (D19).
    pub fn update_all_count(&self) -> usize {
        self.listing.entries().filter(|e| e.updatable()).count()
    }

    /// Updates that "Update all" will skip (held or modified): the tooltip "N held or modified update(s) skipped".
    pub fn update_all_skipped(&self) -> usize {
        self.listing.entries().filter(|e| e.has_update() && !e.updatable()).count()
    }

    /// UI gate: action controls (install, update, remove, ...) are enabled only when no mutating
    /// command runs or waits (a read-only query in flight does not count: a click queues behind
    /// it), `ritz-get` could be spawned (D58) and the listing is not refused (D32).
    pub fn actions_enabled(&self) -> bool {
        (!self.busy || self.busy_queries_only) && !self.ritz_get_missing && self.refused.is_none()
    }

    /// The last `update` run reported this program in its `failed:` line (D58): mark the card.
    pub fn last_failed(&self, e: &Entry) -> bool {
        self.last_run.as_ref().is_some_and(|l| l.failed_names.iter().any(|n| *n == e.name))
    }

    /// Status bar: "last update failed (see log)".
    pub fn last_update_failed(&self) -> bool {
        self.last_run.as_ref().is_some_and(LastRun::has_failures)
    }

    /// Append a log line stamped with the current local time (`HH:MM:SS`). The log keeps the
    /// newest [`LOG_CAP`] lines.
    pub fn push_log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push(LogLine { time: clock_now(), kind, text: text.into() });
        if self.log.len() > LOG_CAP {
            let n = self.log.len() - LOG_CAP;
            self.log.drain(..n);
            self.log_dropped += n;
        }
    }

    /// Clear the one-shot scroll requests after the frame that honoured them.
    pub fn clear_scroll_requests(&mut self) {
        self.scroll_to_side = false;
        self.scroll_to_card = false;
    }

    // ---- per-card progress ------------------------------------------------------------

    /// A tracked command (install / update / switch / reset) starts: its card goes Running
    /// (single command), or every card Update all will touch (the set of [`Self::update_all_count`])
    /// goes Queued, the first of them Running. Replaces the bars of an earlier run; other commands
    /// leave them alone.
    pub fn begin_progress(&mut self, cmd: &Command) {
        match cmd {
            Command::Install { name, .. }
            | Command::Update { name: Some(name) }
            | Command::Switch { name, .. }
            | Command::Reset { name } => {
                self.progress.clear();
                self.progress_order.clear();
                self.marks = Marks::default();
                let verb = self.verb_of(cmd);
                self.progress.insert(name.clone(), CardProgress { phase: Progress::Running, verb, ended: None });
            }
            Command::Update { name: None } => {
                self.progress.clear();
                self.progress_order.clear();
                self.marks = Marks::default();
                let names: Vec<String> = self.listing.entries().filter(|e| e.updatable()).map(|e| e.name.clone()).collect();
                for n in names {
                    if !self.progress.contains_key(&n) {
                        self.progress.insert(n.clone(), CardProgress { phase: Progress::Queued, verb: Verb::Update, ended: None });
                        self.progress_order.push(n);
                    }
                }
                self.promote_progress();
            }
            _ => {}
        }
    }

    /// What `cmd` does to its program (picks the wording of a bar and of the progress dialog).
    pub fn verb_of(&self, cmd: &Command) -> Verb {
        let installed = |name: &str| self.listing.entries().any(|e| e.name == name && !e.is_website() && e.is_installed());
        match cmd {
            Command::Install { name, .. } if installed(name) => Verb::Repair,
            Command::Install { .. } => Verb::Install,
            Command::Switch { .. } => Verb::Switch,
            Command::Reset { .. } => Verb::Reset,
            Command::Remove { .. } => Verb::Remove,
            _ => Verb::Update,
        }
    }

    /// Update all: when nothing runs, the first not-finished program (CLI order) is the running one.
    fn promote_progress(&mut self) {
        if self.progress.values().any(|c| c.phase == Progress::Running) {
            return;
        }
        let next = self.progress_order.iter().find(|n| self.progress[*n].phase == Progress::Queued).cloned();
        if let Some(c) = next.and_then(|n| self.progress.get_mut(&n)) {
            c.phase = Progress::Running;
        }
    }

    /// A streamed `NAME: ...` result line. Only names with a bar matter (held / modified programs
    /// of Update all have none).
    pub fn progress_result(&mut self, name: &str, outcome: Outcome) {
        // Update all, no `#start` yet: an update (or a build failure) can only be printed by the
        // apply step, which a ritz-get with markers announces first. This one has none.
        if !self.progress_order.is_empty()
            && !self.marks.started
            && match &outcome {
                Outcome::Updated { .. } => true,
                Outcome::Failed { reason } => !reason.starts_with("fetch failed"),
                _ => false,
            }
        {
            self.marks.legacy = true;
        }
        let Some(c) = self.progress.get_mut(name).filter(|c| !c.is_final()) else { return };
        c.phase = if matches!(outcome, Outcome::Failed { .. }) { Progress::Failed } else { Progress::Result(outcome) };
        if !self.marks.started {
            self.promote_progress();
        }
    }

    /// `#start NAME`: NAME's apply step begins. For Update all it is the running card, exactly
    /// (what the inference guessed before goes back to Queued; a card still running from an earlier
    /// `#start` that printed no result line is Done).
    pub fn progress_start(&mut self, name: &str) {
        let (n, m) = self.marks.seg;
        if n > 0 && n == m {
            // The segment is complete and a program starts again: ritz-get re-executed itself.
            self.marks.base += m;
            self.marks.seg = (0, 0);
        }
        let first = !self.marks.started;
        self.marks.started = true;
        self.marks.current = Some(name.to_owned());
        if self.progress_order.is_empty() {
            return; // a single command's card is Running from the start
        }
        for c in self.progress.values_mut().filter(|c| c.phase == Progress::Running) {
            c.phase = if first { Progress::Queued } else { Progress::Done };
        }
        if let Some(c) = self.progress.get_mut(name).filter(|c| !c.is_final()) {
            c.phase = Progress::Running;
        }
    }

    /// `#progress NAME N/M`: after NAME's result line, N of M done in this part of the run.
    pub fn progress_count(&mut self, n: usize, m: usize) {
        self.marks.seg = (n, m);
    }

    /// The tracked command finished: settle what has no result line yet and start the fade clock.
    /// `Busy` / `SpawnFailed`: nothing happened, the bars go.
    pub fn finish_progress(&mut self, exit: ExitKind, now: Instant) {
        if matches!(exit, ExitKind::Busy | ExitKind::SpawnFailed) {
            self.progress.clear();
            self.progress_order.clear();
            return;
        }
        let single = self.progress_order.is_empty();
        self.progress.retain(|_, c| {
            if !c.is_final() {
                if exit.is_ok() {
                    c.phase = Progress::Done;
                } else if single {
                    c.phase = Progress::Failed;
                } else {
                    return false; // Update all stopped early: what it never reached shows nothing
                }
            }
            c.ended = Some(now);
            true
        });
        self.progress_order.clear();
    }

    /// Drop finished bars [`PROGRESS_FADE`] after the command ended, but not before the refreshed
    /// listing is in (the card's own button must be right when it comes back). Returns how long
    /// until the next bar is due, for the repaint timer.
    pub fn expire_progress(&mut self, now: Instant) -> Option<std::time::Duration> {
        // The progress dialog's rows are these bars: they stay while it shows (they go once it closes).
        if self.busy || self.loading || (self.modal == Some(Modal::Run) && self.run.is_some()) {
            return None;
        }
        self.progress.retain(|_, c| c.ended.is_none_or(|t| now.duration_since(t) < PROGRESS_FADE));
        self.progress.values().filter_map(|c| c.ended).map(|t| PROGRESS_FADE.saturating_sub(now.duration_since(t))).min()
    }

    /// Some bar animates: the app repaints on a timer (and only then).
    pub fn progress_running(&self) -> bool {
        self.progress.values().any(|c| c.phase == Progress::Running)
    }

    // ---- the progress dialog ----------------------------------------------------------

    /// A command that opens the progress dialog starts (the `$ ritz-get ...` log line is already in).
    /// Not while the refused dialog owns the window (its "Update ritz-get-gui" runs in the background).
    pub fn begin_run(&mut self, cmd: &Command) {
        let at = self.log.len().saturating_sub(1);
        self.begin_run_at(cmd, at);
    }

    /// [`Self::begin_run`] for a command whose `$ ritz-get ...` line is `self.log[at]`.
    fn begin_run_at(&mut self, cmd: &Command, at: usize) {
        if !opens_run(cmd) || self.refused.is_some() {
            return;
        }
        let verb = self.verb_of(cmd);
        self.run = Some(Run {
            cmd: cmd.clone(),
            verb,
            name: cmd.target().map(str::to_owned),
            order: self.progress_order.clone(),
            phase: RunPhase::Running,
            settling: false,
            ask_menu: false,
            change: None,
            log_start: self.log_dropped + at,
        });
        self.run_log_open = false;
        self.modal = Some(Modal::Run);
    }

    /// The run's log lines (its `$ ritz-get ...` line first).
    pub fn run_lines(&self) -> &[LogLine] {
        let Some(run) = &self.run else { return &[] };
        let start = run.log_start.saturating_sub(self.log_dropped).min(self.log.len());
        &self.log[start..]
    }

    /// The newest line the command printed (not the `$ ...` line).
    pub fn run_latest(&self) -> Option<&LogLine> {
        self.run_lines().iter().rev().find(|l| l.kind != LogKind::Cmd)
    }

    /// Update all: (programs done, programs to do). ritz-get's own count once it printed one
    /// (`#progress`, the programs of the apply list only: the ones phase 1 settled are not in it),
    /// else the programs with a final result over the programs in the set.
    pub fn run_counts(&self) -> (usize, usize) {
        if let Some(total) = self.marks.total() {
            return (self.marks.done(), total);
        }
        if self.marks.started && self.marks.base > 0 {
            return (self.marks.base, self.marks.base); // re-exec: the first part is done, the second not counted yet
        }
        let Some(run) = &self.run else { return (0, 0) };
        let total = run.order.len();
        let done = run.order.iter().filter(|n| self.progress.get(*n).is_some_and(CardProgress::is_final)).count();
        (done, total)
    }

    /// "Updating 3 of 5" (the current program's index, capped).
    pub fn run_position(&self) -> (usize, usize) {
        let (done, total) = self.run_counts();
        ((done + 1).min(total), total)
    }

    /// Update all: is "N of M" real? Yes with ritz-get's `#progress` count, and for an older ritz-get
    /// (no markers: the count of the set). No while it still checks for updates (phase 1: no
    /// `#start` yet) and between the first `#start` and the first `#progress`.
    pub fn run_counted(&self) -> bool {
        self.marks.total().is_some() || (!self.marks.started && self.marks.legacy)
    }

    /// The headline of a running Update all (the dialog's title and the reopen button).
    pub fn run_all_title(&self) -> String {
        if self.run_counted() {
            let (n, m) = self.run_position();
            format!("Updating {n} of {m}\u{2026}")
        } else if let Some(cur) = self.marks.current.as_deref().filter(|_| self.marks.started) {
            format!("Updating {}\u{2026}", self.display_of(cur))
        } else {
            "Checking for updates\u{2026}".to_owned()
        }
    }

    /// The program's list name for display (`NAME` when unknown).
    pub fn display_of(&self, name: &str) -> String {
        self.listing.entries().find(|e| e.name == name).map_or_else(|| name.to_owned(), |e| e.display().to_owned())
    }

    /// Set the final phase of the run (exit 3 detail, ...) and show the dialog when it needs the user.
    pub fn set_run_phase(&mut self, phase: RunPhase) {
        if let Some(run) = &mut self.run {
            run.phase = phase;
            run.settling = false;
        }
        self.after_run_phase();
    }

    /// The run reached a final phase: a result that needs the user opens (or reopens) the dialog;
    /// a plain success while it is backgrounded just goes. A dialog the user is looking at stays.
    fn after_run_phase(&mut self) {
        let Some(run) = &self.run else { return };
        let needs = match &run.phase {
            RunPhase::Running => return,
            RunPhase::Done { menu } => *menu,
            _ => true,
        };
        if needs {
            if matches!(self.modal, None | Some(Modal::Run)) {
                self.modal = Some(Modal::Run);
            }
        } else if self.modal != Some(Modal::Run) {
            self.run = None;
        }
    }

    /// The command ended (not exit 3: [`App`] fills that in with the tools). Ok waits for the refresh.
    pub fn finish_run(&mut self, command: &Command, exit: ExitKind, captured: &Captured) {
        let Some(run) = self.run.as_ref().filter(|r| r.running() && r.cmd == *command) else { return };
        let reason = || {
            captured
                .results
                .iter()
                .find_map(|r| match &r.outcome {
                    Outcome::Failed { reason } => Some(reason.clone()),
                    _ => None,
                })
                .or_else(|| self.run_lines().iter().rev().find(|l| l.kind == LogKind::Err).map(|l| l.text.clone()))
        };
        let phase = match exit {
            ExitKind::Ok => {
                let change = run.name.as_deref().and_then(|n| {
                    captured.results.iter().find_map(|r| match &r.outcome {
                        Outcome::Updated { from, to } if r.name == n => Some((from.clone(), to.clone())),
                        _ => None,
                    })
                });
                if let Some(run) = &mut self.run {
                    run.settling = true;
                    run.change = change;
                }
                return;
            }
            ExitKind::Deps => return,
            ExitKind::NeedsRoot => RunPhase::Setup,
            ExitKind::Busy => RunPhase::Busy,
            ExitKind::Failed => RunPhase::Failed { reason: reason().unwrap_or_else(|| "it failed".into()) },
            ExitKind::Usage => RunPhase::Failed { reason: "usage error (exit 2)".into() },
            ExitKind::SpawnFailed => {
                self.run = None;
                if self.modal == Some(Modal::Run) {
                    self.modal = None;
                }
                return;
            }
            ExitKind::Other(n) => RunPhase::Failed { reason: reason().unwrap_or_else(|| format!("exited with code {n}")) },
        };
        self.set_run_phase(phase);
    }

    /// The online listing is in: a run that ended well is now done.
    pub fn settle_run(&mut self) {
        let Some(run) = self.run.as_mut().filter(|r| r.settling) else { return };
        run.settling = false;
        run.phase = RunPhase::Done { menu: run.ask_menu };
        self.after_run_phase();
    }

    /// While the dialog is hidden: what the "show it again" button says. `None` = nothing to show.
    pub fn reopen_label(&self) -> Option<String> {
        let run = self.run.as_ref().filter(|_| self.modal != Some(Modal::Run))?;
        let who = run.name.as_deref().map(|n| self.display_of(n));
        Some(match (&run.phase, &who) {
            (RunPhase::Running, None) => self.run_all_title(),
            (RunPhase::Running, Some(w)) => format!("{} {w}\u{2026}", run.verb.gerund()),
            (RunPhase::Done { .. }, None) => "Update finished".into(),
            (RunPhase::Done { .. }, Some(w)) => format!("{w}: add to the start menu?"),
            (RunPhase::Failed { .. }, None) => "Update failed".into(),
            (RunPhase::Failed { .. }, Some(w)) => format!("{w}: {} failed", run.verb.base()),
            (RunPhase::Deps { .. }, _) => "Missing tools".into(),
            (RunPhase::Setup, _) => "Setup needed".into(),
            (RunPhase::Busy, _) => "ritz-get is busy".into(),
        })
    }

    /// Apply the D57 screenshot spec (`RITZ_GET_GUI_SHOT`): `default`, `select=<section/name>`,
    /// `menu=<section/name>`, `settings`, `reset=<section/name>`, `deps=<section/name>`, `busy`,
    /// `restart`, `refused`, `progress` (a frozen mid-way Update all: per-card bars, busy), `progress-single=<section/name>` (that card installing / updating); `startmenu=<section/name>` (the D47 "Add to the start menu?" modal, that entry shown as freshly installed); `busy` is a single `update ts6tray` running (its card Running), `remove=<section/name>` (selects it, then the app itself runs `ritz-get remove`); plus `setupneeded=<section/name>` (exit-4 badge on that entry) and (I14) `deps-none=<section/name>` (Deps modal without a `deps:`
    /// command), `needs` (several NEEDS / SETUP NEEDED badges), `logerr` (a log with FAILED / busy
    /// lines) and `empty` (a search that matches nothing). Sets initial UI state only; `default` and
    /// unknown specs do nothing. `menu=` selects the card and sets [`AppState::open_menu`].
    ///
    /// Not hooks, because the real flow produces them (`scripts/shot.sh` sets them up): the
    /// "ritz-get not found" banner (`--ritz-get` pointing at a bogus path) and "last update failed"
    /// (`ritz-get/last-run` in the sandboxed cache dir, see `--cache-seed`).
    pub fn apply_shot_hook(&mut self, spec: &str) {
        let (key, arg) = spec.split_once('=').map_or((spec, ""), |(k, a)| (k, a));
        let id = || arg.to_owned();
        match key {
            "select" => {
                self.selected = Some(id());
                self.scroll_to_side = true;
                self.scroll_to_card = true;
            }
            "menu" => {
                self.selected = Some(id());
                self.open_menu = Some(id());
                self.scroll_to_card = true;
            }
            "setupneeded" => {
                // Exit-4 state: the side panel's "Setup needed" card + the SETUP NEEDED pill.
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                self.badges.insert(name, Badge::SetupNeeded);
                self.selected = Some(id());
                self.scroll_to_side = true;
                self.scroll_to_card = true;
            }
            "settings" => self.view = View::Settings,
            "reset" => {
                self.selected = Some(id());
                self.modal = Some(Modal::Reset { id: id() });
            }
            "deps" | "deps-none" | "install-deps" => {
                // Exit 3 inside the progress dialog. `deps-none`: an unknown distro (PORCELAIN: no `deps:`
                // line, raw `missing:` lines), so no Copy command.
                let (missing, command): (Vec<String>, Option<String>) = if key == "deps-none" {
                    (
                        vec![
                            "missing: cargo (try: pacman:rust dnf:cargo apt:cargo)".into(),
                            "missing: pkg-config (try: pacman:pkgconf dnf:pkgconf-pkg-config apt:pkg-config)".into(),
                        ],
                        None,
                    )
                } else {
                    (
                        vec![
                            "missing: cargo (try: pacman:rust dnf:cargo apt:cargo)".into(),
                            "missing: pkg-config (try: pacman:pkgconf dnf:pkgconf-pkg-config apt:pkg-config)".into(),
                        ],
                        Some("sudo pacman -S --needed rust pkgconf".into()),
                    )
                };
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                self.badges.insert(name.clone(), Badge::Needs(vec!["cargo".into(), "pkg-config".into()]));
                let lines = [
                    (LogKind::Info, format!("Cloning {name} (main) ...")),
                    (LogKind::Err, "missing: cargo (try: pacman:rust dnf:cargo apt:cargo)".to_owned()),
                    (LogKind::Err, "missing: pkg-config (try: pacman:pkgconf dnf:pkgconf-pkg-config apt:pkg-config)".to_owned()),
                ];
                self.shot_run(arg, Command::Install { name, branch: None }, &lines, Some(ExitKind::Deps));
                self.set_run_phase(RunPhase::Deps { missing, command });
            }
            "install-running" => {
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                let lines = [
                    (LogKind::Info, format!("Cloning {name} (main) ...")),
                    (LogKind::Info, format!("{name}: checking tools")),
                    (LogKind::Info, format!("{name}: building")),
                ];
                self.shot_run(arg, Command::Install { name, branch: None }, &lines, None);
            }
            "install-hidden" | "updateall-hidden" => {
                // "Run in background" was pressed: no dialog; the status bar / top bar offer the way back.
                let again = if key == "install-hidden" { format!("install-running={arg}") } else { "updateall-running".to_owned() };
                self.apply_shot_hook(&again);
                self.modal = None;
            }
            "install-failed" => {
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                let lines = [
                    (LogKind::Info, format!("Cloning {name} (main) ...")),
                    (LogKind::Info, format!("{name}: building")),
                    (LogKind::Err, format!("{name}: FAILED (build failed: cargo exited with 101)")),
                    (LogKind::Err, "FAILED \u{2014} your installed version is unchanged and still works".to_owned()),
                ];
                self.shot_run(arg, Command::Install { name, branch: None }, &lines, Some(ExitKind::Failed));
                self.set_run_phase(RunPhase::Failed { reason: "build failed: cargo exited with 101".into() });
            }
            "install-done" | "startmenu" => {
                // A fresh install whose refreshed listing says the program is installed, shortcut `off`:
                // the Done state is the "Add it to the start menu?" card (2a).
                if let Some(e) = self.listing.sections.iter_mut().flat_map(|s| s.entries.iter_mut()).find(|e| e.id() == arg) {
                    e.state = State::Installed;
                    e.status = Status::Current;
                    e.installed = e.latest.clone();
                    e.behind = Some(0);
                    e.dirty = Some(0);
                    e.ahead = Some(0);
                    e.shortcut = ShortcutState::Off;
                }
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                let lines = [
                    (LogKind::Info, format!("Cloning {name} (main) ...")),
                    (LogKind::Info, format!("{name}: building")),
                    (LogKind::Ok, format!("{name}: installed")),
                ];
                // `begin_progress` must see it as not installed yet: start the run before the edit above? No:
                // the verb is only used for wording, so say Install explicitly.
                self.shot_run(arg, Command::Install { name, branch: None }, &lines, Some(ExitKind::Ok));
                if let Some(run) = &mut self.run {
                    run.verb = Verb::Install;
                    run.ask_menu = true;
                    run.settling = true;
                }
                self.settle_run();
            }
            "log" => {
                self.apply_shot_hook("logerr");
                self.modal = Some(Modal::Log);
            }
            "updateall-running" => {
                self.shot_progress_all();
                let at = self.log.iter().rposition(|l| l.kind == LogKind::Cmd).unwrap_or(0);
                self.begin_run_at(&Command::Update { name: None }, at);
            }
            "updateall-done" => {
                self.shot_progress_all();
                let at = self.log.iter().rposition(|l| l.kind == LogKind::Cmd).unwrap_or(0);
                // the two that were still going end well
                let to = Outcome::Updated { from: "2026-10-18 91ac4f6".into(), to: "2026-10-23 c40d2a8".into() };
                self.progress_result("ritz-get-gui", to);
                self.progress_result("ccorral", Outcome::Updated { from: "2026-10-19 7d1e0b2".into(), to: "2026-10-23 e55f6a7".into() });
                self.progress_count(5, 5);
                self.progress_result("ts6tray", Outcome::Updated { from: "2026-10-20 a596443".into(), to: "2026-10-22 b12c3d4".into() });
                self.push_log(LogKind::Ok, "ritz-get-gui: updated 2026-10-18 91ac4f6 -> 2026-10-23 c40d2a8");
                self.push_log(LogKind::Ok, "ccorral: updated 2026-10-19 7d1e0b2 -> 2026-10-23 e55f6a7");
                self.push_log(LogKind::Ok, "ts6tray: updated 2026-10-20 a596443 -> 2026-10-22 b12c3d4");
                self.push_log(LogKind::Info, "4 updated, 1 up to date, 1 skipped, 1 failed");
                let order = self.progress_order.clone(); // listing order; `finish_progress` clears it
                self.finish_progress(ExitKind::Failed, Instant::now());
                self.busy = false;
                self.running = None;
                self.progress_order = order;
                self.begin_run_at(&Command::Update { name: None }, at);
                self.set_run_phase(RunPhase::Failed { reason: "1 program failed".into() });
            }
            "needs" => {
                // Session badges as after exit 3 / 4: several `Needs` pills (they wrap) and SETUP NEEDED.
                let needs = |tools: &[&str]| Badge::Needs(tools.iter().map(|t| (*t).to_owned()).collect());
                self.badges.insert("ForzaTelemetryV3Launcher".into(), needs(&["cargo", "cmake", "pkg-config"]));
                self.badges.insert("simple-screensharing".into(), needs(&["protoc"]));
                self.badges.insert("geofinder".into(), needs(&["cargo", "pkg-config"]));
                self.badges.insert("ts6tray".into(), Badge::SetupNeeded);
                self.selected = Some("Software/ForzaTelemetryV3Launcher".into());
                self.scroll_to_side = true;
                self.scroll_to_card = true;
            }
            "logerr" => {
                // The log after a failed `update` (exit 1) followed by a busy refusal, idle.
                self.push_log(LogKind::Cmd, "$ ritz-get update --yes");
                self.push_log(LogKind::Info, "Checking for updates ...");
                self.push_log(LogKind::Ok, "ritz-get: updated 2026-10-01 aaa1111 -> 2026-10-14 3f9e21a");
                self.push_log(LogKind::Err, "geofinder: FAILED (install.sh exit 1)");
                self.push_log(LogKind::Err, "FAILED \u{2014} your installed version is unchanged and still works");
                self.push_log(LogKind::Info, "1 updated, 0 skipped, 1 failed");
                self.push_log(LogKind::Note, "`ritz-get update --yes` failed: see the FAILED lines above");
                self.push_log(LogKind::Cmd, "$ ritz-get update --yes");
                self.push_log(LogKind::Err, "busy: another ritz-get is running");
            }
            "empty" => self.search = "zzz-no-match".into(),
            "busy" => {
                // A single `ritz-get update ts6tray` running: its card's Running bar, every other
                // button disabled, the side panel's Actions row, a matching log.
                let cmd = Command::Update { name: Some("ts6tray".into()) };
                self.begin_progress(&cmd);
                self.selected = Some("Helpers/ts6tray".into());
                self.scroll_to_side = true;
                self.scroll_to_card = true;
                self.busy = true;
                self.push_log(LogKind::Cmd, format!("$ ritz-get {}", cmd.args().join(" ")));
                self.push_log(LogKind::Info, "ts6tray: updating ...");
                self.push_log(LogKind::Info, "ts6tray: building ...");
                self.running = Some(cmd);
            }
            "progress" => self.shot_progress_all(),
            "progress-single" => {
                // `ritz-get update|install NAME` running for the selected card: its bar, and the
                // side panel's Actions row.
                let name = arg.rsplit('/').next().unwrap_or(arg).to_owned();
                let installed = self.listing.entries().any(|e| e.name == name && e.is_installed());
                let cmd = if installed { Command::Update { name: Some(name) } } else { Command::Install { name, branch: None } };
                self.begin_progress(&cmd);
                self.selected = Some(id());
                self.scroll_to_side = true;
                self.scroll_to_card = true;
                self.busy = true;
                self.push_log(LogKind::Cmd, format!("$ ritz-get {}", cmd.args().join(" ")));
                self.push_log(LogKind::Info, "Building ...");
                self.running = Some(cmd);
            }
            "restart" => self.restart = true,
            "refused" => self.refused = Some(2),
            _ => {}
        }
    }

    /// The `progress` hook: a frozen mid-way Update all (also the base of `updateall-running` / `-done`).
    fn shot_progress_all(&mut self) {
        // A frozen mid-way Update all, in listing order (only programs before the running one are
        // finished). Each bar sits on a card whose pill agrees: geofinder (modified, Skipped),
        // SteamAlt (behind, FAILED), gamescope-ritz (current, Up to date), ritz-get (behind,
        // Updated); ritz-get-gui (behind) Running; ccorral, ts6tray (behind) Queued. Nothing expires (`ended` stays None while `busy`).
        use Progress as P;
        let phases = [
            ("geofinder", P::Result(Outcome::SkippedLocalChanges)),
            ("SteamAlt", P::Failed),
            ("gamescope-ritz", P::Result(Outcome::UpToDate)),
            ("ritz-get", P::Result(Outcome::Updated { from: "2026-10-20 a596443".into(), to: "2026-10-22 b12c3d4".into() })),
            ("ritz-get-gui", P::Running),
            ("ccorral", P::Queued),
            ("ts6tray", P::Queued),
        ];
        for e in self.listing.sections.iter_mut().flat_map(|s| s.entries.iter_mut()) {
            if matches!(e.name.as_str(), "SteamAlt" | "ccorral" | "ritz-get") {
                e.status = Status::Behind; // pending in the update set
                e.behind = Some(2);
                e.latest = Version::parse("2026-10-23 e55f6a7");
            }
        }
        self.progress.clear();
        self.progress_order.clear();
        // ritz-get's markers: the two phase-1 results (geofinder, gamescope-ritz) are not in the apply
        // list of 5; 2 are done and ritz-get-gui is the one running.
        self.marks = Marks { started: true, legacy: false, current: Some("ritz-get-gui".into()), base: 0, seg: (2, 5) };
        // listing order, as the CLI processes them
        let names: Vec<String> = self.listing.entries().map(|e| e.name.clone()).collect();
        for n in names {
            if let Some((_, phase)) = phases.iter().find(|(p, _)| *p == n) {
                self.progress.insert(n.clone(), CardProgress { phase: phase.clone(), verb: Verb::Update, ended: None });
                self.progress_order.push(n);
            }
        }
        self.filter = Filter::Installed; // dense grid: the first cards show every kind of bar
        self.busy = true;
        self.running = Some(Command::Update { name: None });
        self.push_log(LogKind::Cmd, "$ ritz-get update --yes");
        self.push_log(LogKind::Info, "geofinder: SKIPPED (local changes; ritz-get reset geofinder to discard them)");
        self.push_log(LogKind::Err, "SteamAlt: FAILED (install.sh exit 1)");
        self.push_log(LogKind::Info, "gamescope-ritz: up to date");
        self.push_log(LogKind::Ok, "ritz-get: updated 2026-10-20 a596443 -> 2026-10-22 b12c3d4");
        self.push_log(LogKind::Info, "ritz-get-gui: updating ...");
    
    }

    /// A run for the progress-dialog hooks: select the entry, start `cmd` (card bar, `$` line, `lines`,
    /// the dialog). `end`: the exit it ended with (`None` = still running, busy).
    fn shot_run(&mut self, id: &str, cmd: Command, lines: &[(LogKind, String)], end: Option<ExitKind>) {
        self.selected = Some(id.to_owned());
        self.scroll_to_side = true;
        self.scroll_to_card = true;
        self.begin_progress(&cmd);
        self.push_log(LogKind::Cmd, format!("$ {cmd}"));
        self.begin_run(&cmd);
        for (k, l) in lines {
            self.push_log(*k, l.clone());
        }
        match end {
            None => {
                self.busy = true;
                self.running = Some(cmd);
            }
            Some(exit) => self.finish_progress(exit, Instant::now()),
        }
    }

    /// Apply the actions that only change UI state. Returns the action back if it needs `App`
    /// (commands, clipboard, xdg-open, restart).
    fn apply_ui_action(&mut self, a: Action) -> Option<Action> {
        match a {
            Action::Select(id) => {
                let changed = self.selected.as_deref() != Some(id.as_str());
                self.scroll_to_side |= changed;
                self.scroll_to_card |= changed;
                self.selected = Some(id);
            }
            Action::ToggleGroup(name) => {
                if !self.collapsed.remove(&name) {
                    self.collapsed.insert(name);
                }
            }
            Action::SetFilter(f) => self.filter = f,
            Action::SetSearch(s) => self.search = s,
            Action::SetSort(s) => self.sort = s,
            Action::SetTouchMode(on) => self.touch_mode = on,
            Action::ShowSettings => self.view = View::Settings,
            Action::ShowMain => self.view = View::Main,
            Action::ToggleRunLog => self.run_log_open = !self.run_log_open,
            Action::ShowModal(Modal::Run) => {
                if self.run.is_some() {
                    self.modal = Some(Modal::Run);
                }
            }
            Action::ShowModal(m) => self.modal = Some(m),
            Action::CloseModal => {
                // Esc / backdrop / Close on the progress dialog: while it runs that sends it to the
                // background; once it is over, the run is dismissed.
                if self.modal == Some(Modal::Run) && self.run.as_ref().is_some_and(|r| !r.running()) {
                    self.run = None;
                }
                self.modal = None;
            }
            other => return Some(other),
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers: clock, persistence, output classification
// ---------------------------------------------------------------------------------------------

/// The log keeps the newest this-many lines.
const LOG_CAP: usize = 5000;

/// Self-update: `update` result for this program sets [`AppState::restart`] (D25).
const SELF_NAME: &str = "ritz-get-gui";

/// glibc/musl `struct tm` (Linux). Only used by [`clock_now`].
#[repr(C)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: std::ffi::c_long,
    tm_zone: *const std::ffi::c_char,
}

unsafe extern "C" {
    fn localtime_r(t: *const std::ffi::c_long, out: *mut Tm) -> *mut Tm;
}

/// Local time `HH:MM:SS` (no chrono dependency: libc's `localtime_r`, which std already links).
fn clock_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let t = secs as std::ffi::c_long;
    let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
    // SAFETY: `t` and `tm` are valid for the call; localtime_r is thread-safe and fills `tm`
    // (a zeroed `Tm` is a valid value, the pointer field may be null).
    let ok = unsafe { !localtime_r(&t, tm.as_mut_ptr()).is_null() };
    if ok {
        // SAFETY: initialised above (zeroed, then filled by localtime_r).
        let tm = unsafe { tm.assume_init() };
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    } else {
        format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60)
    }
}

fn nonempty_path(v: Option<std::ffi::OsString>) -> Option<PathBuf> {
    v.filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// `$XDG_CONFIG_HOME/ritz-get-gui/settings`, else `$HOME/.config/ritz-get-gui/settings` (D43).
fn config_file_from(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let base = nonempty_path(xdg).or_else(|| nonempty_path(home).map(|h| h.join(".config")))?;
    Some(base.join("ritz-get-gui").join("settings"))
}

/// `$XDG_CACHE_HOME/ritz-get/last-run`, else `$HOME/.cache/ritz-get/last-run` (D58).
fn last_run_file_from(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let base = nonempty_path(xdg).or_else(|| nonempty_path(home).map(|h| h.join(".cache")))?;
    Some(base.join("ritz-get").join("last-run"))
}

/// The persisted sort (`sort=<key>` line), `Sort::Default` when the file or line is missing.
pub fn load_sort(path: &Path) -> Sort {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("sort=").map(|k| Sort::from_key(k.trim()))))
        .unwrap_or_default()
}

/// Write `sort=<key>`, keeping any other lines of the settings file.
pub fn save_sort(path: &Path, sort: Sort) -> std::io::Result<()> {
    save_key(path, "sort", sort.key())
}

/// The persisted touch mode (`touch=1`), off when the file or line is missing.
pub fn load_touch(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .is_some_and(|t| t.lines().any(|l| l.trim().strip_prefix("touch=").is_some_and(|v| v.trim() == "1")))
}

/// Write `touch=0|1`, keeping any other lines of the settings file.
pub fn save_touch(path: &Path, on: bool) -> std::io::Result<()> {
    save_key(path, "touch", if on { "1" } else { "0" })
}

/// Replace (or append) the `key=value` line, keeping every other line.
fn save_key(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let prefix = format!("{key}=");
    let mut out: Vec<String> =
        old.lines().filter(|l| !l.trim_start().starts_with(&prefix)).map(str::to_owned).collect();
    out.push(format!("{prefix}{value}"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, out.join("\n") + "\n")
}

/// Parse `~/.cache/ritz-get/last-run`: `<ISO time> N updated, N skipped, N failed` plus an
/// optional `failed: NAME NAME` line. `None` when the first line does not look like that.
pub fn parse_last_run(text: &str) -> Option<LastRun> {
    let mut lines = text.lines();
    let first = lines.next()?.trim();
    let (time, rest) = first.split_once(char::is_whitespace)?;
    if !rest.contains("updated") {
        return None;
    }
    let num = |kw: &str| -> u32 {
        rest.split(',')
            .find_map(|part| {
                let (n, w) = part.trim().split_once(' ')?;
                if w.trim() == kw { n.parse().ok() } else { None }
            })
            .unwrap_or(0)
    };
    let failed_names = lines
        .find_map(|l| l.trim().strip_prefix("failed:").map(|r| r.split_whitespace().map(str::to_owned).collect()))
        .unwrap_or_default();
    Some(LastRun {
        time: time.to_owned(),
        updated: num("updated"),
        skipped: num("skipped"),
        failed: num("failed"),
        failed_names,
    })
}

/// `dir` / `shortcuts` print one path; read tolerantly: first non-empty line, trimmed (D58).
fn first_path_line(stdout: &str) -> Option<PathBuf> {
    stdout.lines().map(str::trim).find(|l| !l.is_empty()).map(PathBuf::from)
}

/// Commands whose cards get a progress bar.
fn tracked(c: &Command) -> bool {
    matches!(c, Command::Install { .. } | Command::Update { .. } | Command::Switch { .. } | Command::Reset { .. })
}

fn exit_code(e: ExitKind) -> i32 {
    match e {
        ExitKind::Ok => 0,
        ExitKind::Failed => 1,
        ExitKind::Usage => 2,
        ExitKind::Deps => 3,
        ExitKind::NeedsRoot => 4,
        ExitKind::Busy => 5,
        ExitKind::SpawnFailed => -127,
        ExitKind::Other(n) => n,
    }
}

/// `NAME: updated A -> B`
fn is_updated_line(l: &str) -> bool {
    l.split_once(": updated ").is_some_and(|(_, rest)| rest.contains("->") || rest.contains('\u{2192}'))
}

/// Log colour of one streamed line (keep it simple, D32): FAILED / errors red, `NAME: updated`
/// and shortcut results green, `needs root:` / `deps:` / "Additional steps" accent, rest plain.
fn classify(line: &str) -> LogKind {
    let l = line.trim();
    let lower = l.to_lowercase();
    if l.contains("FAILED")
        || lower.starts_with("error")
        || lower.starts_with("fatal")
        || lower.starts_with("busy:")
        || lower.starts_with("missing:")
    {
        LogKind::Err
    } else if lower.starts_with("needs root:") || lower.starts_with("deps:") || l.starts_with("Additional steps") {
        LogKind::Note
    } else if is_updated_line(l) || l.contains("shortcut created") || l.contains("shortcut removed") {
        LogKind::Ok
    } else {
        LogKind::Info
    }
}

/// `missing: TOOL (try: pacman:PKG ...)` -> `TOOL`.
fn tool_of_missing(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("missing:")?.trim();
    let tool = rest.split(|c: char| c.is_whitespace() || c == '(').next()?;
    (!tool.is_empty()).then(|| tool.to_owned())
}

/// `needs: a, b` (the `FAILED (needs: a, b)` reason) -> `[a, b]`.
fn tools_of_reason(reason: &str) -> Option<Vec<String>> {
    let i = reason.find("needs:")?;
    let list = reason[i + "needs:".len()..].trim().trim_end_matches(')');
    Some(list.split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect())
}

fn dedup(v: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------------------------

type ParseFn = fn(&str) -> Result<Listing, PorcelainError>;

/// The eframe app.
pub struct App {
    pub state: AppState,
    runner: Runner,
    icons: IconFetcher,
    ctx: Option<egui::Context>,
    /// Commands waiting for the runner (FIFO); the next starts on `Finished`.
    queue: VecDeque<Command>,
    /// The command the runner is executing right now.
    in_flight: Option<Command>,
    /// `RITZ_GET_GUI_SHOT` spec, applied once the listing is in (see [`App::try_apply_shot`]).
    pending_shot: Option<String>,
    /// D47: NAME of a program whose first `install` just succeeded; the next online listing decides
    /// whether to ask "Add NAME to the start menu?" (column 17 `off`). Cleared by that listing.
    offer_shortcut: Option<String>,
    /// A listing has been parsed at least once.
    listed: bool,
    /// Injectable for tests; `porcelain::parse` in the app.
    parse: ParseFn,
    /// `~/.config/ritz-get-gui/settings`; `None` = do not persist (tests).
    config_path: Option<PathBuf>,
    /// `~/.cache/ritz-get/last-run`; `None` = do not read (tests).
    last_run_path: Option<PathBuf>,
    /// Binary to run instead of `$RITZ_GET` (tests drive the real runner against the fake CLI through this).
    bin: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        crate::theme::install_fonts(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut app = App::build(Some(cc.egui_ctx.clone()));
        let var = |k: &str| std::env::var_os(k);
        app.config_path = config_file_from(var("XDG_CONFIG_HOME"), var("HOME"));
        app.last_run_path = last_run_file_from(var("XDG_CACHE_HOME"), var("HOME"));
        app.pending_shot = std::env::var("RITZ_GET_GUI_SHOT").ok().filter(|s| !s.is_empty());
        app.load_persisted();
        app.begin_startup();
        app
    }

    /// An `App` that touches neither disk nor the display (unit tests build on this).
    fn build(ctx: Option<egui::Context>) -> App {
        App {
            state: AppState::default(),
            runner: Runner::new(),
            icons: IconFetcher::new(),
            ctx,
            queue: VecDeque::new(),
            in_flight: None,
            pending_shot: None,
            offer_shortcut: None,
            listed: false,
            parse: porcelain::parse,
            config_path: None,
            last_run_path: None,
            bin: None,
        }
    }

    /// Sort (D43) and last-run (D58) from disk.
    fn load_persisted(&mut self) {
        if let Some(p) = &self.config_path {
            self.state.sort = load_sort(p);
            self.state.touch_mode = load_touch(p);
        }
        self.reload_last_run();
    }

    fn reload_last_run(&mut self) {
        if let Some(p) = &self.last_run_path {
            self.state.last_run = std::fs::read_to_string(p).ok().and_then(|t| parse_last_run(&t));
        }
    }

    /// D6: queue `dir`, `list --offline --porcelain`, `list --porcelain`, `shortcuts`
    /// (strictly one at a time; see the module docs for why this order).
    pub fn begin_startup(&mut self) {
        self.queue.clear();
        self.state.ritz_get_missing = false;
        self.state.refused = None;
        self.queue.push_back(Command::Dir { path: None });
        self.queue.push_back(Command::List { offline: true, all: false });
        self.queue.push_back(Command::List { offline: false, all: false });
        self.queue.push_back(Command::Shortcuts { path: None });
        self.sync_loading();
    }

    fn sync_loading(&mut self) {
        let is_list = |c: &Command| matches!(c, Command::List { .. });
        self.state.loading = self.in_flight.as_ref().is_some_and(is_list) || self.queue.iter().any(is_list);
        self.state.busy_queries_only = self.state.busy
            && self.in_flight.as_ref().is_some_and(Command::is_query)
            && self.queue.iter().all(Command::is_query);
    }

    /// Queue a command (identical queued queries are not duplicated).
    fn enqueue(&mut self, cmd: Command) {
        if cmd.is_query() && self.queue.contains(&cmd) {
            return;
        }
        self.queue.push_back(cmd);
        self.sync_loading();
    }

    /// If nothing runs, take the next queued command and mark it in flight (`busy`, `running`,
    /// the `$ ritz-get ...` log line for non-queries). The caller hands it to the runner.
    fn next_to_start(&mut self) -> Option<Command> {
        if self.in_flight.is_some() {
            return None;
        }
        let cmd = self.queue.pop_front()?;
        if !cmd.is_query() {
            self.state.push_log(LogKind::Cmd, format!("$ {cmd}"));
        }
        self.in_flight = Some(cmd.clone());
        self.state.begin_progress(&cmd);
        self.state.begin_run(&cmd);
        self.state.busy = true;
        self.state.running = Some(cmd.clone());
        self.sync_loading();
        Some(cmd)
    }

    /// The runner refused `cmd`: put it back at the front.
    fn unstart(&mut self, cmd: Command) {
        if !cmd.is_query() {
            self.state.log.pop();
        }
        self.in_flight = None;
        self.state.busy = false;
        self.state.running = None;
        if tracked(&cmd) {
            self.state.finish_progress(ExitKind::SpawnFailed, Instant::now()); // clears the bars
        }
        if self.state.run.as_ref().is_some_and(|r| r.cmd == cmd) {
            self.state.run = None;
            if self.state.modal == Some(Modal::Run) {
                self.state.modal = None;
            }
        }
        self.queue.push_front(cmd);
        self.sync_loading();
    }

    fn pump(&mut self, ctx: &egui::Context) {
        if let Some(cmd) = self.next_to_start() {
            let started = match self.bin.clone() {
                Some(bin) => self.runner.start_with(bin, cmd.clone(), ctx.clone()),
                None => self.runner.start(cmd.clone(), ctx.clone()),
            };
            if !started {
                self.unstart(cmd);
            }
        }
    }

    /// Everything a runner event can change. The only place runner output touches the state.
    pub fn handle_event(&mut self, ev: RunnerEvent) {
        match ev {
            RunnerEvent::Line { text, stream } => {
                let t = text.trim_end();
                if !t.is_empty() {
                    let machine = stream == Stream::Stdout && runner::is_machine_line(t);
                    if machine {
                        // `#start` / `#progress` (and any future `#...` line) drive the bars and are never shown.
                        match runner::parse_marker(t) {
                            Some(Marker::Start { name }) => self.state.progress_start(&name),
                            Some(Marker::Progress { n, m, .. }) => self.state.progress_count(n, m),
                            None => {}
                        }
                    } else {
                        self.state.push_log(classify(t), t);
                    }
                    if !machine && !self.state.progress.is_empty() {
                        // The same parser the final `Captured` uses: one result line = one program done.
                        let mut one = Captured::default();
                        one.absorb(t, stream);
                        for r in one.results {
                            self.state.progress_result(&r.name, r.outcome);
                        }
                    }
                }
            }
            RunnerEvent::Finished { command, exit, captured } => self.finished(command, exit, captured),
        }
        self.try_apply_shot();
    }

    fn finished(&mut self, command: Command, exit: ExitKind, captured: Captured) {
        self.in_flight = None;
        self.state.busy = false;
        self.state.running = None;
        if tracked(&command) {
            self.state.finish_progress(exit, Instant::now());
        }

        if exit == ExitKind::SpawnFailed {
            // D58: banner + actions disabled; the rest of the queue would fail the same way.
            self.state.ritz_get_missing = true;
            self.state.finish_run(&command, exit, &captured);
            self.queue.clear();
            self.state.push_log(LogKind::Err, "ritz-get not found: install it, then press Retry");
            self.sync_loading();
            return;
        }
        self.state.ritz_get_missing = false;

        match &command {
            Command::List { offline, .. } => self.list_finished(*offline, exit, &captured),
            Command::Dir { path: None } => {
                if let (true, Some(p)) = (exit.is_ok(), first_path_line(&captured.stdout)) {
                    self.state.dir = Some(p);
                    self.request_icons();
                } else {
                    self.state.push_log(LogKind::Err, format!("`{command}` failed (exit {})", exit_code(exit)));
                }
            }
            Command::Shortcuts { path: None } => {
                if let (true, Some(p)) = (exit.is_ok(), first_path_line(&captured.stdout)) {
                    self.state.shortcuts_dir = Some(p);
                } else {
                    self.state.push_log(LogKind::Err, format!("`{command}` failed (exit {})", exit_code(exit)));
                }
            }
            Command::Branches { name } => {
                if exit.is_ok() {
                    self.state.branches.insert(name.clone(), runner::branches(&captured.stdout));
                } else {
                    self.state.push_log(LogKind::Err, format!("`{command}` failed (exit {})", exit_code(exit)));
                }
            }
            _ => self.mutation_finished(&command, exit, &captured),
        }
        self.sync_loading();
    }

    fn list_finished(&mut self, offline: bool, exit: ExitKind, captured: &Captured) {
        self.list_read(offline, exit, captured);
        // A run that ended well is done once the refreshed (online) listing is in, ok or not.
        if !offline {
            self.state.settle_run();
        }
    }

    fn list_read(&mut self, offline: bool, exit: ExitKind, captured: &Captured) {
        // D47: an online listing (ok or not) settles a pending start-menu question; an offline one does not.
        let offer = if offline { None } else { self.offer_shortcut.take() };
        if !exit.is_ok() {
            self.state.push_log(LogKind::Err, format!("`list` failed (exit {})", exit_code(exit)));
            return;
        }
        match (self.parse)(&captured.stdout) {
            Ok(listing) => {
                let reached = !offline && listing.online != Some(false);
                // An offline listing never replaces a fresher online one.
                if !(offline && self.state.checked_at.is_some()) {
                    self.state.listing = listing;
                }
                if reached {
                    self.state.checked_at = Some(Instant::now());
                }
                self.listed = true;
                if let Some(name) = offer {
                    self.offer_start_menu(&name);
                }
                let gone = self.state.selected.as_deref().is_some_and(|id| self.state.listing.find(id).is_none());
                if gone {
                    self.state.selected = None;
                }
                self.request_icons();
            }
            Err(PorcelainError::UnsupportedVersion(n)) => {
                self.state.refused = Some(n);
                self.queue.retain(|c| !matches!(c, Command::List { .. }));
                self.state.push_log(
                    LogKind::Err,
                    format!("ritz-get speaks porcelain {n}, this ritz-get-gui only understands 1: update ritz-get-gui"),
                );
            }
            Err(e) => {
                self.state.push_log(LogKind::Err, format!("could not read the listing: {e}"));
            }
        }
    }

    /// D47: the progress dialog of a fresh install asks "Add it to the start menu?" when the refreshed
    /// listing shows the program with a `.desktop` that is not in the start menu yet (column 17 `off`).
    fn offer_start_menu(&mut self, name: &str) {
        if self.state.refused.is_some() {
            return;
        }
        let ask = self
            .state
            .listing
            .entries()
            .any(|e| e.name == name && !e.is_website() && e.is_installed() && e.shortcut == ShortcutState::Off);
        if let Some(run) = self.state.run.as_mut().filter(|r| r.settling && r.name.as_deref() == Some(name)) {
            run.ask_menu = ask;
        }
    }

    /// An `install`/`update`/`remove`/... finished (D32, D48, D58, D61).
    fn mutation_finished(&mut self, command: &Command, exit: ExitKind, captured: &Captured) {
        self.state.finish_run(command, exit, captured);
        // D47: only a successful `install` of a program that was NOT installed before (`Repair` is the
        // same `install NAME` on an installed one) may lead to the start-menu question.
        self.offer_shortcut = match command {
            Command::Install { name, .. } if exit.is_ok() => {
                let before = self.state.listing.entries().find(|e| e.name == *name && !e.is_website());
                (!before.is_some_and(Entry::is_installed)).then(|| name.clone())
            }
            _ => None,
        };
        // The refused dialog's "Update ritz-get-gui" (D32): afterwards start over, which clears `refused`.
        let refused_fix =
            self.state.refused.is_some() && matches!(command, Command::Update { name: Some(n) } if n == SELF_NAME);
        if matches!(command, Command::Update { .. }) {
            let self_updated = captured
                .results
                .iter()
                .any(|r| r.name == SELF_NAME && matches!(r.outcome, Outcome::Updated { .. }));
            if self_updated {
                self.state.restart = true;
            }
            self.reload_last_run();
        }

        for p in self.icons_to_refresh(command, exit, captured) {
            icons::forget_scaled(&p);
        }

        match exit {
            ExitKind::Ok => {
                // A clean run clears the session badge of what it touched.
                if let Some(t) = command.target() {
                    self.state.badges.remove(t);
                }
                for r in captured.results.iter().filter(|r| matches!(r.outcome, Outcome::Updated { .. })) {
                    self.state.badges.remove(&r.name);
                }
            }
            ExitKind::Failed => {
                // The FAILED lines were streamed already; D61: a failed reset keeps MODIFIED (the refresh shows it).
                self.state.push_log(LogKind::Note, format!("`{command}` failed: see the FAILED lines above"));
            }
            ExitKind::Usage => {
                self.state.push_log(LogKind::Err, format!("`{command}`: usage error (exit 2)"));
            }
            ExitKind::Deps => self.deps_needed(command, captured),
            ExitKind::NeedsRoot => self.setup_needed(command, captured),
            ExitKind::Busy => {
                // The CLI's own stderr line already streamed in (exactly one busy line); only add ours when it did not.
                if !self.state.log.last().is_some_and(|l| l.text.contains("another ritz-get is running")) {
                    self.state.push_log(LogKind::Err, "busy: another ritz-get is running");
                }
                return; // nothing changed: no refresh
            }
            ExitKind::SpawnFailed => {}
            ExitKind::Other(n) => {
                self.state.push_log(LogKind::Err, format!("`{command}` exited with code {n}"));
            }
        }

        // Re-read what is installed. `dir PATH` / `shortcuts PATH` re-query their folder first.
        match command {
            Command::Dir { path: Some(_) } => self.enqueue(Command::Dir { path: None }),
            Command::Shortcuts { path: Some(_) } => self.enqueue(Command::Shortcuts { path: None }),
            _ => {}
        }
        if refused_fix {
            self.begin_startup();
        } else if !matches!(command, Command::Open { .. }) {
            self.enqueue(Command::List { offline: false, all: false });
        }
    }

    /// `icon.png` paths of clones a finished command may have changed: a successful install (repair),
    /// switch or reset of `NAME`, and every program an `update` reported as `updated`. Never `remove`
    /// (the last-good icon must survive the uninstall). Their cached textures/bytes are dropped so the
    /// new file shows without a restart (the GUI's own update re-execs and needs none of this).
    fn icons_to_refresh(&self, command: &Command, exit: ExitKind, captured: &Captured) -> Vec<PathBuf> {
        let Some(dir) = self.state.dir.as_deref().filter(|_| exit.is_ok()) else { return Vec::new() };
        let names: Vec<&str> = match command {
            Command::Install { name, .. } | Command::Switch { name, .. } | Command::Reset { name } => vec![name],
            Command::Update { .. } => captured
                .results
                .iter()
                .filter(|r| matches!(r.outcome, Outcome::Updated { .. }))
                .map(|r| r.name.as_str())
                .collect(),
            _ => Vec::new(),
        };
        names.into_iter().map(|n| dir.join(n).join("icon.png")).collect()
    }

    /// Exit 3: NEEDS badges + the Deps modal.
    fn deps_needed(&mut self, command: &Command, captured: &Captured) {
        let from_missing = dedup(captured.missing.iter().filter_map(|l| tool_of_missing(l)));
        let mut affected: Vec<(String, Vec<String>)> = Vec::new();
        for r in &captured.results {
            if let Outcome::Failed { reason } = &r.outcome {
                if let Some(tools) = tools_of_reason(reason) {
                    affected.push((r.name.clone(), tools));
                }
            }
        }
        if affected.is_empty() {
            if let Some(n) = command.target() {
                affected.push((n.to_owned(), Vec::new()));
            }
        }
        let mut all_tools = Vec::new();
        for (name, tools) in &affected {
            let tools = if tools.is_empty() { from_missing.clone() } else { tools.clone() };
            all_tools.extend(tools.iter().cloned());
            self.state.badges.insert(name.clone(), Badge::Needs(tools));
        }
        let all_tools = dedup(if all_tools.is_empty() { from_missing } else { all_tools });

        let text = if all_tools.is_empty() {
            "a program needs tools that are missing".to_owned()
        } else {
            format!("missing tools: {}", all_tools.join(", "))
        };
        self.state.push_log(LogKind::Note, text);

        if self.state.run.as_ref().is_some_and(|r| r.running() && r.cmd == *command) {
            let missing = if captured.missing.is_empty() {
                all_tools.iter().map(|t| format!("missing: {t}")).collect()
            } else {
                captured.missing.clone()
            };
            self.state.set_run_phase(RunPhase::Deps { missing, command: captured.deps.clone() });
        }
    }

    /// Exit 4: SETUP NEEDED badge + terminal hint.
    fn setup_needed(&mut self, command: &Command, captured: &Captured) {
        let mut names: Vec<String> = Vec::new();
        if let Some(t) = command.target() {
            names.push(t.to_owned());
        } else {
            for r in &captured.results {
                if matches!(&r.outcome, Outcome::Failed { reason } if reason.contains("root")) {
                    names.push(r.name.clone());
                }
            }
            if names.is_empty() {
                for l in &captured.needs_root {
                    let first = l.split_whitespace().next().unwrap_or("").trim_end_matches(':');
                    if self.state.listing.entries().any(|e| e.name == first) {
                        names.push(first.to_owned());
                    }
                }
            }
        }
        let names = dedup(names);
        for n in &names {
            self.state.badges.insert(n.clone(), Badge::SetupNeeded);
            self.state.push_log(LogKind::Note, format!("{n}: run `ritz-get install {n}` in a terminal"));
        }
        if names.is_empty() {
            self.state.push_log(LogKind::Note, "run `ritz-get install NAME` in a terminal");
        }
    }

    /// D57: apply the screenshot spec once it can resolve ids: after the first listing, or once
    /// startup is idle (nothing queued or running) if no listing ever arrived. `busy` waits for idle,
    /// because the real startup commands would otherwise clear the busy state again (`startmenu`'s
    /// in-memory edit of the listing would be overwritten by the refresh).
    fn try_apply_shot(&mut self) {
        let Some(spec) = self.pending_shot.as_deref() else { return };
        let idle = self.in_flight.is_none() && self.queue.is_empty();
        let waits = ["progress", "startmenu", "install-", "updateall", "log"];
        let ready = if spec == "busy" || waits.iter().any(|w| spec.starts_with(w)) { idle } else { self.listed || idle };
        if ready {
            if let Some(spec) = self.pending_shot.take() {
                self.state.apply_shot_hook(&spec);
                // `remove=<section/name>`: select the entry and run the real `ritz-get remove` (icon-flicker shots).
                if let Some(id) = spec.strip_prefix("remove=") {
                    self.state.selected = Some(id.to_owned());
                    self.state.scroll_to_card = true;
                    self.run_user_command(Command::Remove { name: id.rsplit('/').next().unwrap_or(id).to_owned() });
                }
            }
        }
    }

    fn request_icons(&mut self) {
        let Some(ctx) = self.ctx.clone() else { return };
        let dir = self.state.dir.clone();
        for e in self.state.listing.entries() {
            self.icons.request(e, dir.as_deref(), &ctx);
        }
    }

    /// Run one action from the UI.
    fn dispatch(&mut self, ctx: &egui::Context, action: Action) {
        if let (Action::SetSort(s), Some(p)) = (&action, &self.config_path) {
            if let Err(e) = save_sort(p, *s) {
                self.state.push_log(LogKind::Err, format!("could not save the sort order: {e}"));
            }
        }
        if let (Action::SetTouchMode(on), Some(p)) = (&action, &self.config_path) {
            if let Err(e) = save_touch(p, *on) {
                self.state.push_log(LogKind::Err, format!("could not save touch mode: {e}"));
            }
        }
        ctx.request_repaint(); // the state changes below show up on the next frame
        let Some(action) = self.state.apply_ui_action(action) else { return };
        match action {
            Action::OpenUrl(target) | Action::OpenFolder(target) => self.open_detached(&target),
            Action::CopyToClipboard(text) => ctx.copy_text(text),
            Action::CopyLog => {
                let text: Vec<String> = self.state.log.iter().map(|l| format!("{} {}", l.time, l.text)).collect();
                ctx.copy_text(text.join("\n"));
            }
            Action::Restart => self.restart_self(),
            Action::Retry => self.begin_startup(),
            other => {
                if let Some(cmd) = other.to_command() {
                    self.run_user_command(cmd);
                }
            }
        }
    }

    /// Queue a command the user asked for. Queries (list, branches, dir/shortcuts without a path)
    /// may queue while busy; anything that changes things is dropped unless actions are enabled.
    ///
    /// One exception: while the listing is refused (`#porcelain` version unsupported, D32) the
    /// refused dialog's "Update ritz-get-gui" must run, or the app could never fix itself.
    fn run_user_command(&mut self, cmd: Command) {
        let self_update = matches!(&cmd, Command::Update { name: Some(n) } if n == SELF_NAME);
        let refused_fix = self_update && self.state.refused.is_some() && !self.state.busy && !self.state.ritz_get_missing;
        if !cmd.is_query() && !self.state.actions_enabled() && !refused_fix {
            self.state.push_log(LogKind::Note, format!("not started (busy or unavailable): {cmd}"));
            return;
        }
        self.enqueue(cmd);
    }

    /// `xdg-open TARGET`, detached; not a ritz-get command, so only a Note in the log.
    fn open_detached(&mut self, target: &str) {
        use std::process::Stdio;
        let spawned = std::process::Command::new("xdg-open")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                self.state.push_log(LogKind::Note, format!("opening {target}"));
            }
            Err(e) => self.state.push_log(LogKind::Err, format!("xdg-open failed: {e}")),
        }
    }

    /// D25: re-exec `~/.local/bin/ritz-get-gui`, else the current executable. Returns only on error.
    fn restart_self(&mut self) {
        use std::os::unix::process::CommandExt;
        let installed = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/ritz-get-gui"));
        let target = installed.filter(|p| p.exists()).or_else(|| std::env::current_exe().ok());
        match target {
            Some(t) => {
                let err = std::process::Command::new(&t).exec();
                self.state.push_log(LogKind::Err, format!("restart failed ({}): {err}", t.display()));
            }
            None => self.state.push_log(LogKind::Err, "restart failed: cannot find ritz-get-gui"),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for ev in self.runner.poll() {
            self.handle_event(ev);
        }
        self.pump(ctx);
        let due = self.state.expire_progress(Instant::now());

        let had_menu = self.state.open_menu.is_some();
        let mut actions = Vec::new();
        crate::ui::draw(ctx, &self.state, &mut actions);
        self.state.clear_scroll_requests(); // one frame is enough to scroll
        if had_menu {
            self.state.open_menu = None; // one frame is enough to open the popover
        }
        for a in actions {
            self.dispatch(ctx, a);
        }
        self.pump(ctx);
        // Repaint only while a bar animates (~30 fps) or is due to fade; idle = no repaints.
        if self.state.progress_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if let Some(d) = due {
            ctx.request_repaint_after(d + std::time::Duration::from_millis(20));
        }
    }
}

/// Whole-app flows against the fake CLI: call-log and exit-code assertions (I14).
#[cfg(test)]
#[path = "flow_tests.rs"]
mod flow_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{ProgramResult, RunSummary, Stream};
    use std::time::Duration;

    /// Stand-in for `porcelain::parse` (I2 lands separately): `#porcelain 1` -> the sample listing.
    fn fake_parse(text: &str) -> Result<Listing, PorcelainError> {
        let first = text.lines().next().unwrap_or("");
        match first.strip_prefix("#porcelain ").and_then(|n| n.trim().parse::<u32>().ok()) {
            Some(1) => {
                let mut l = crate::sample::listing();
                l.online = Some(!text.contains("#online 0"));
                Ok(l)
            }
            Some(n) => Err(PorcelainError::UnsupportedVersion(n)),
            None => Err(PorcelainError::MissingHeader),
        }
    }

    fn app() -> App {
        let mut a = App::build(None);
        a.parse = fake_parse;
        a
    }

    fn fin(cmd: Command, exit: ExitKind, cap: Captured) -> RunnerEvent {
        RunnerEvent::Finished { command: cmd, exit, captured: cap }
    }

    fn out(s: &str) -> Captured {
        Captured { stdout: s.into(), ..Captured::default() }
    }

    fn list(offline: bool) -> Command {
        Command::List { offline, all: false }
    }

    fn some_name(a: &App) -> (String, String) {
        let e = a.state.listing.entries().find(|e| e.is_installed()).unwrap();
        (e.name.clone(), e.id())
    }

    /// An app with the sample listing loaded through the event path.
    fn loaded() -> App {
        let mut a = app();
        a.begin_startup();
        while let Some(c) = a.next_to_start() {
            let cap = match &c {
                Command::List { offline, .. } => out(&format!("#porcelain 1\n#online {}\n", !offline as u8)),
                Command::Dir { .. } => out("/home/x/.local/share/ritz-get\n"),
                _ => out("/home/x/.local/share/applications\n"),
            };
            a.handle_event(fin(c, ExitKind::Ok, cap));
        }
        a
    }

    #[test]
    fn startup_order_and_one_at_a_time() {
        let mut a = app();
        a.begin_startup();
        assert!(a.state.loading);
        let order = [
            Command::Dir { path: None },
            list(true),
            list(false),
            Command::Shortcuts { path: None },
        ];
        for (i, want) in order.iter().enumerate() {
            let got = a.next_to_start().expect("next command");
            assert_eq!(&got, want);
            assert!(a.state.busy);
            assert_eq!(a.state.running.as_ref(), Some(want));
            // Nothing else may start before Finished.
            assert_eq!(a.next_to_start(), None, "second start while {want} runs");
            assert_eq!(a.next_to_start(), None);
            let cap = if matches!(want, Command::List { .. }) { out("#porcelain 1\n#online 0\n") } else { out("/p\n") };
            a.handle_event(fin(got, ExitKind::Ok, cap));
            assert!(!a.state.busy);
            if i == 1 {
                // The offline listing shows up as soon as it parses, still checking for the online one.
                assert!(!a.state.listing.sections.is_empty());
                assert!(a.state.loading);
                assert!(a.state.checked_at.is_none());
            }
        }
        assert_eq!(a.next_to_start(), None);
        assert!(!a.state.loading);
        assert_eq!(a.state.dir, Some(PathBuf::from("/p")));
        assert_eq!(a.state.shortcuts_dir, Some(PathBuf::from("/p")));
    }

    #[test]
    fn online_list_sets_checked_at_and_offline_never_replaces_it() {
        let mut a = loaded();
        assert!(a.state.checked_at.is_some());
        assert!(a.listed);
        let n = a.state.listing.entries().count();
        a.state.listing.sections.clear();
        a.handle_event(fin(list(true), ExitKind::Ok, out("#porcelain 1\n#online 0\n")));
        assert!(a.state.listing.sections.is_empty(), "offline listing must not replace an online one");
        a.handle_event(fin(list(false), ExitKind::Ok, out("#porcelain 1\n#online 1\n")));
        assert_eq!(a.state.listing.entries().count(), n);
    }

    #[test]
    fn dir_output_is_read_tolerantly() {
        let mut a = app();
        a.handle_event(fin(Command::Dir { path: None }, ExitKind::Ok, out("\n  /srv/ritz  \nsecond\n")));
        assert_eq!(a.state.dir, Some(PathBuf::from("/srv/ritz")));
    }

    #[test]
    fn exit3_badge_and_deps_modal_with_deps_line() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        let cmd = Command::Install { name: name.clone(), branch: None };
        a.queue.push_back(cmd.clone());
        assert_eq!(a.next_to_start(), Some(cmd.clone()));
        assert_eq!(a.state.modal, Some(Modal::Run), "a tracked command opens the progress dialog");
        let cap = Captured {
            missing: vec!["missing: cargo (try: pacman:rust dnf:cargo apt:cargo)".into(), "missing: pkg-config (try: pacman:pkgconf)".into()],
            deps: Some("sudo pacman -S --needed rust pkgconf".into()),
            ..Captured::default()
        };
        a.handle_event(fin(cmd, ExitKind::Deps, cap.clone()));
        assert_eq!(a.state.badges.get(&name), Some(&Badge::Needs(vec!["cargo".into(), "pkg-config".into()])));
        assert_eq!(a.state.modal, Some(Modal::Run));
        assert_eq!(
            a.state.run.as_ref().map(|r| r.phase.clone()),
            Some(RunPhase::Deps { missing: cap.missing.clone(), command: Some("sudo pacman -S --needed rust pkgconf".into()) })
        );
        assert!(a.queue.contains(&list(false)), "refresh after a mutation");
    }

    #[test]
    fn exit3_without_deps_line_and_needs_from_failed_results() {
        let mut a = loaded();
        let names: Vec<String> = a.state.listing.entries().filter(|e| e.is_installed()).map(|e| e.name.clone()).take(2).collect();
        let cap = Captured {
            missing: vec!["missing: cargo (try: pacman:rust)".into()],
            results: vec![
                ProgramResult { name: names[0].clone(), outcome: Outcome::Failed { reason: "needs: cargo, make".into() } },
                ProgramResult { name: names[1].clone(), outcome: Outcome::UpToDate },
            ],
            ..Captured::default()
        };
        a.queue.push_back(Command::Update { name: None });
        a.next_to_start();
        a.handle_event(fin(Command::Update { name: None }, ExitKind::Deps, cap));
        assert_eq!(a.state.badges.get(&names[0]), Some(&Badge::Needs(vec!["cargo".into(), "make".into()])));
        assert!(!a.state.badges.contains_key(&names[1]));
        match a.state.run.as_ref().map(|r| &r.phase) {
            Some(RunPhase::Deps { missing, command }) => {
                assert_eq!(missing, &vec!["missing: cargo (try: pacman:rust)".to_owned()]);
                assert_eq!(command, &None);
            }
            m => panic!("expected the Missing tools state, got {m:?}"),
        }
    }

    #[test]
    fn exit4_setup_needed() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        let cap = Captured { needs_root: vec![format!("needs root: {name} wants a udev rule")], ..Captured::default() };
        let cmd = Command::Install { name: name.clone(), branch: None };
        a.queue.push_back(cmd.clone());
        a.next_to_start();
        a.handle_event(fin(cmd, ExitKind::NeedsRoot, cap));
        assert_eq!(a.state.badges.get(&name), Some(&Badge::SetupNeeded));
        let last = a.state.log.iter().find(|l| l.text.contains("in a terminal")).expect("hint");
        assert!(last.text.contains(&format!("ritz-get install {name}")));
        assert_eq!(a.state.run.as_ref().map(|r| r.phase.clone()), Some(RunPhase::Setup), "shown in the dialog's result area");
        assert_eq!(a.state.modal, Some(Modal::Run));
    }

    #[test]
    fn exit5_clears_busy_and_logs() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        let cmd = Command::Hold { name };
        a.queue.push_back(cmd.clone());
        assert_eq!(a.next_to_start(), Some(cmd.clone()));
        assert!(a.state.busy);
        a.handle_event(fin(cmd.clone(), ExitKind::Busy, Captured::default()));
        assert!(!a.state.busy && a.state.running.is_none());
        assert!(a.state.actions_enabled());
        assert_eq!(a.state.log.last().unwrap().text, "busy: another ritz-get is running");
        assert_eq!(a.state.log.last().unwrap().kind, LogKind::Err);
        assert!(a.queue.is_empty(), "no refresh after busy");

        // The CLI already printed its line: still exactly one.
        let mut b = loaded();
        b.queue.push_back(cmd.clone());
        assert_eq!(b.next_to_start(), Some(cmd.clone()));
        b.state.push_log(LogKind::Err, "busy: another ritz-get is running"); // the streamed stderr line
        b.handle_event(fin(cmd, ExitKind::Busy, Captured::default()));
        let n = b.state.log.iter().filter(|l| l.text.contains("another ritz-get is running")).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn exit1_adds_note_and_refreshes() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        a.handle_event(fin(Command::Reset { name }, ExitKind::Failed, Captured::default()));
        assert_eq!(a.state.log.last().unwrap().kind, LogKind::Note);
        assert_eq!(a.queue.front(), Some(&list(false)));
    }

    #[test]
    fn spawn_failed_disables_actions() {
        let mut a = app();
        a.begin_startup();
        let c = a.next_to_start().unwrap();
        a.handle_event(fin(c, ExitKind::SpawnFailed, Captured::default()));
        assert!(a.state.ritz_get_missing);
        assert!(!a.state.actions_enabled());
        assert!(a.queue.is_empty());
        assert!(!a.state.loading);
        // Retry restarts the startup.
        a.dispatch(&egui::Context::default(), Action::Retry);
        assert!(!a.state.ritz_get_missing);
        assert_eq!(a.queue.len(), 4);
    }

    #[test]
    fn unsupported_porcelain_is_refused() {
        let mut a = app();
        a.begin_startup();
        let _ = a.next_to_start();
        a.handle_event(fin(Command::Dir { path: None }, ExitKind::Ok, out("/p")));
        let c = a.next_to_start().unwrap();
        a.handle_event(fin(c, ExitKind::Ok, out("#porcelain 2\n")));
        assert_eq!(a.state.refused, Some(2));
        assert!(!a.state.actions_enabled());
        assert!(a.state.listing.sections.is_empty());
        assert!(!a.queue.iter().any(|c| matches!(c, Command::List { .. })), "no online list after a refusal");
    }

    /// The refused dialog's "Update ritz-get-gui" is the one command that runs while refused; when
    /// it finishes the app starts over (which clears `refused`).
    #[test]
    fn refused_dialog_update_runs_then_restarts_startup() {
        let ctx = egui::Context::default();
        let mut a = app();
        a.begin_startup();
        while let Some(c) = a.next_to_start() {
            let cap = if matches!(c, Command::List { .. }) { out("#porcelain 2\n") } else { out("/p\n") };
            a.handle_event(fin(c, ExitKind::Ok, cap));
        }
        assert_eq!(a.state.refused, Some(2));

        // Nothing else mutates while refused ...
        a.dispatch(&ctx, Action::Hold { name: "foo".into() });
        a.dispatch(&ctx, Action::Update { name: None });
        a.dispatch(&ctx, Action::Update { name: Some("other".into()) });
        assert!(a.queue.is_empty(), "only the self-update may run: {:?}", a.queue);
        // ... and not while something else is running either.
        a.state.busy = true;
        a.dispatch(&ctx, Action::Update { name: Some(SELF_NAME.into()) });
        assert!(a.queue.is_empty());
        a.state.busy = false;

        a.dispatch(&ctx, Action::Update { name: Some(SELF_NAME.into()) });
        let upd = Command::Update { name: Some(SELF_NAME.into()) };
        assert_eq!(a.next_to_start(), Some(upd.clone()));
        let cap = Captured {
            results: vec![ProgramResult {
                name: SELF_NAME.into(),
                outcome: Outcome::Updated { from: "a".into(), to: "b".into() },
            }],
            ..Captured::default()
        };
        a.handle_event(fin(upd, ExitKind::Ok, cap));
        assert_eq!(a.state.refused, None, "startup is re-run, which clears the refusal");
        assert!(a.state.restart, "the self-update asks for a restart");
        assert_eq!(
            a.queue.iter().cloned().collect::<Vec<_>>(),
            vec![Command::Dir { path: None }, list(true), list(false), Command::Shortcuts { path: None }]
        );
    }

    /// A busy exit (another ritz-get holds the lock) leaves the refused dialog up so it can be retried.
    #[test]
    fn refused_update_busy_keeps_the_dialog() {
        let mut a = app();
        a.state.refused = Some(2);
        let upd = Command::Update { name: Some(SELF_NAME.into()) };
        a.queue.push_back(upd.clone());
        assert_eq!(a.next_to_start(), Some(upd.clone()));
        a.handle_event(fin(upd, ExitKind::Busy, Captured::default()));
        assert_eq!(a.state.refused, Some(2));
        assert!(a.queue.is_empty());
    }

    /// Selecting a different card asks the stacked layout to scroll to the side panel; re-selecting
    /// the same one (a card's button pushes `Select` too) does not.
    #[test]
    fn selecting_a_new_card_requests_the_side_scroll() {
        let ctx = egui::Context::default();
        let mut a = app();
        a.dispatch(&ctx, Action::Select("Software/a".into()));
        assert!(a.state.scroll_to_side);
        a.state.scroll_to_side = false;
        a.dispatch(&ctx, Action::Select("Software/a".into()));
        assert!(!a.state.scroll_to_side);
        a.dispatch(&ctx, Action::Select("Software/b".into()));
        assert!(a.state.scroll_to_side);
    }

    /// The groups pane scrolls to a newly selected card (one-shot); re-selecting does not ask again.
    #[test]
    fn selecting_a_new_card_requests_the_card_scroll() {
        let ctx = egui::Context::default();
        let mut a = app();
        assert!(!a.state.scroll_to_card);
        a.dispatch(&ctx, Action::Select("Software/a".into()));
        assert!(a.state.scroll_to_card);
        a.state.clear_scroll_requests();
        assert!(!a.state.scroll_to_card && !a.state.scroll_to_side);
        a.dispatch(&ctx, Action::Select("Software/a".into()));
        assert!(!a.state.scroll_to_card);
        a.dispatch(&ctx, Action::Select("Software/b".into()));
        assert!(a.state.scroll_to_card);
    }

    #[test]
    fn shot_hook_selection_and_setupneeded_scroll_to_the_card() {
        for spec in ["select=Helpers/ts6tray", "menu=Helpers/ts6tray", "setupneeded=Helpers/ts6tray", "needs"] {
            let mut s = AppState::default();
            s.apply_shot_hook(spec);
            assert!(s.scroll_to_card, "{spec}");
        }
        let mut s = AppState::default();
        s.apply_shot_hook("setupneeded=Software/geofinder");
        assert_eq!(s.selected.as_deref(), Some("Software/geofinder"));
        assert_eq!(s.badges.get("geofinder"), Some(&Badge::SetupNeeded));
    }

    #[test]
    fn other_parse_errors_keep_the_previous_listing() {
        let mut a = loaded();
        let n = a.state.listing.entries().count();
        a.handle_event(fin(list(false), ExitKind::Ok, out("garbage")));
        assert_eq!(a.state.listing.entries().count(), n);
        assert_eq!(a.state.refused, None);
        assert_eq!(a.state.log.last().unwrap().kind, LogKind::Err);
    }

    #[test]
    fn last_run_parsing() {
        let l = parse_last_run("2026-10-24T03:00:01 2 updated, 1 skipped, 0 failed\n").unwrap();
        assert_eq!(
            l,
            LastRun { time: "2026-10-24T03:00:01".into(), updated: 2, skipped: 1, failed: 0, failed_names: vec![] }
        );
        assert!(!l.has_failures());
        let l = parse_last_run("2026-10-24T03:00:01 0 updated, 0 skipped, 2 failed\nfailed: foo bar\n").unwrap();
        assert_eq!((l.updated, l.skipped, l.failed), (0, 0, 2));
        assert_eq!(l.failed_names, vec!["foo".to_owned(), "bar".to_owned()]);
        assert!(parse_last_run("").is_none());
        assert!(parse_last_run("nonsense").is_none());
    }

    #[test]
    fn last_run_marks_cards_and_status() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        assert!(!a.state.last_update_failed());
        a.state.last_run = parse_last_run(&format!("2026-10-24T03:00:01 0 updated, 0 skipped, 1 failed\nfailed: {name}\n"));
        assert!(a.state.last_update_failed());
        let e = a.state.listing.entries().find(|e| e.name == name).unwrap();
        assert!(a.state.last_failed(e));
        let other = a.state.listing.entries().find(|e| e.name != name).unwrap();
        assert!(!a.state.last_failed(other));
    }

    #[test]
    fn last_run_reloaded_after_update() {
        let dir = std::env::temp_dir().join(format!("rgg-lastrun-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("last-run");
        std::fs::write(&p, "2026-10-24T03:00:01 1 updated, 0 skipped, 1 failed\nfailed: x\n").unwrap();
        let mut a = loaded();
        a.last_run_path = Some(p);
        a.handle_event(fin(Command::Update { name: None }, ExitKind::Failed, Captured::default()));
        assert_eq!(a.state.last_run.as_ref().unwrap().failed_names, vec!["x".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_paths_and_sort_round_trip() {
        use std::ffi::OsString;
        let o = |s: &str| Some(OsString::from(s));
        assert_eq!(config_file_from(o("/xdg"), o("/home/u")), Some(PathBuf::from("/xdg/ritz-get-gui/settings")));
        assert_eq!(config_file_from(None, o("/home/u")), Some(PathBuf::from("/home/u/.config/ritz-get-gui/settings")));
        assert_eq!(config_file_from(o(""), o("/home/u")), Some(PathBuf::from("/home/u/.config/ritz-get-gui/settings")));
        assert_eq!(last_run_file_from(o("/c"), o("/h")), Some(PathBuf::from("/c/ritz-get/last-run")));
        assert_eq!(last_run_file_from(None, o("/h")), Some(PathBuf::from("/h/.cache/ritz-get/last-run")));

        let dir = std::env::temp_dir().join(format!("rgg-settings-{}", std::process::id()));
        let p = config_file_from(Some(dir.clone().into_os_string()), None).unwrap();
        assert_eq!(load_sort(&p), Sort::Default); // missing file
        for s in Sort::ALL {
            save_sort(&p, s).unwrap();
            assert_eq!(load_sort(&p), s);
        }
        // Other lines survive a save, and the app persists on SetSort.
        std::fs::write(&p, "other=1\nsort=name\n").unwrap();
        let mut a = app();
        a.config_path = Some(p.clone());
        a.load_persisted();
        assert_eq!(a.state.sort, Sort::Name);
        a.dispatch(&egui::Context::default(), Action::SetSort(Sort::UpdatesFirst));
        assert_eq!(a.state.sort, Sort::UpdatesFirst);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("other=1") && text.contains("sort=updates") && !text.contains("sort=name"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_mode_round_trips_and_keeps_other_lines() {
        let dir = std::env::temp_dir().join(format!("rgg-touch-{}", std::process::id()));
        let p = dir.join("settings");
        assert!(!load_touch(&p), "missing file = off");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&p, "other=1\nsort=name\nunknown line\n").unwrap();
        save_touch(&p, true).unwrap();
        assert!(load_touch(&p));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("other=1") && text.contains("sort=name") && text.contains("unknown line") && text.contains("touch=1"));
        save_sort(&p, Sort::UpdatesFirst).unwrap();
        assert!(load_touch(&p), "saving the sort keeps touch");
        save_touch(&p, false).unwrap();
        assert!(!load_touch(&p));
        assert_eq!(std::fs::read_to_string(&p).unwrap().matches("touch=").count(), 1);
        let mut a = app();
        a.config_path = Some(p.clone());
        a.dispatch(&egui::Context::default(), Action::SetTouchMode(true));
        assert!(a.state.touch_mode && load_touch(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shot_hook_variants() {
        let mut s = AppState::sample();
        s.apply_shot_hook("default");
        assert_eq!(s.modal, None);
        assert!(s.selected.as_deref() == Some("Helpers/ts6tray"), "default leaves the selection alone");

        let mut s = AppState::default();
        s.apply_shot_hook("select=Software/foo");
        assert_eq!(s.selected.as_deref(), Some("Software/foo"));
        assert_eq!(s.open_menu, None);

        let mut s = AppState::default();
        s.apply_shot_hook("menu=Software/foo");
        assert_eq!(s.selected.as_deref(), Some("Software/foo"));
        assert_eq!(s.open_menu.as_deref(), Some("Software/foo"));

        let mut s = AppState::default();
        s.apply_shot_hook("settings");
        assert_eq!(s.view, View::Settings);

        let mut s = AppState::default();
        s.apply_shot_hook("reset=Software/foo");
        assert_eq!(s.modal, Some(Modal::Reset { id: "Software/foo".into() }));

        let mut s = AppState::default();
        s.apply_shot_hook("deps=Software/foo");
        assert_eq!(s.modal, Some(Modal::Run));
        assert!(matches!(s.run.as_ref().map(|r| &r.phase), Some(RunPhase::Deps { missing, command: Some(_) }) if !missing.is_empty()));

        let mut s = AppState::default();
        s.apply_shot_hook("busy");
        assert!(s.busy && s.running.is_some() && s.log.len() >= 3);
        assert_eq!(s.progress["ts6tray"].phase, Progress::Running); // the one card the log talks about

        let mut s = AppState::default();
        s.apply_shot_hook("restart");
        assert!(s.restart);

        let mut s = AppState::default();
        s.apply_shot_hook("refused");
        assert_eq!(s.refused, Some(2));

        let mut s = AppState::default();
        s.apply_shot_hook("deps-none=Software/foo");
        assert!(matches!(s.run.as_ref().map(|r| &r.phase), Some(RunPhase::Deps { missing, command: None }) if missing.len() == 2));

        let mut s = AppState::default();
        s.apply_shot_hook("startmenu=Software/foo");
        assert_eq!(s.modal, Some(Modal::Run));
        assert_eq!(s.run.as_ref().map(|r| r.phase.clone()), Some(RunPhase::Done { menu: true }));
        assert_eq!(s.selected.as_deref(), Some("Software/foo"));

        let mut s = AppState::default();
        s.apply_shot_hook("needs");
        assert!(matches!(s.badges.get("ForzaTelemetryV3Launcher"), Some(Badge::Needs(t)) if t.len() == 3));
        assert_eq!(s.badges.get("ts6tray"), Some(&Badge::SetupNeeded));

        let mut s = AppState::default();
        s.apply_shot_hook("logerr");
        assert!(s.log.iter().any(|l| l.kind == LogKind::Err && l.text.contains("FAILED")) && !s.busy);

        let mut s = AppState::default();
        s.apply_shot_hook("empty");
        assert_eq!(s.search, "zzz-no-match");

        let mut s = AppState::default();
        s.apply_shot_hook("bogus=1");
        assert!(s.selected.is_none() && s.modal.is_none() && !s.busy && !s.restart);
    }

    #[test]
    fn shot_is_deferred_until_the_listing_is_in() {
        let mut a = app();
        a.pending_shot = Some("select=Helpers/ts6tray".into());
        a.begin_startup();
        let c = a.next_to_start().unwrap(); // dir
        a.handle_event(fin(c, ExitKind::Ok, out("/p")));
        assert_eq!(a.state.selected, None);
        let c = a.next_to_start().unwrap(); // list --offline
        a.handle_event(fin(c, ExitKind::Ok, out("#porcelain 1\n#online 0\n")));
        assert_eq!(a.state.selected.as_deref(), Some("Helpers/ts6tray"));
        assert!(a.pending_shot.is_none());
    }

    #[test]
    fn busy_shot_waits_for_idle_startup() {
        let mut a = app();
        a.pending_shot = Some("busy".into());
        a.begin_startup();
        while let Some(c) = a.next_to_start() {
            assert!(!a.state.busy || a.pending_shot.is_some());
            let cap = if matches!(c, Command::List { .. }) { out("#porcelain 1\n") } else { out("/p") };
            a.handle_event(fin(c, ExitKind::Ok, cap));
        }
        assert!(a.state.busy);
        assert!(matches!(a.state.running, Some(Command::Update { name: Some(_) })));
    }

    #[test]
    fn shot_applies_even_if_ritz_get_is_missing() {
        let mut a = app();
        a.pending_shot = Some("settings".into());
        a.begin_startup();
        let c = a.next_to_start().unwrap();
        a.handle_event(fin(c, ExitKind::SpawnFailed, Captured::default()));
        assert_eq!(a.state.view, View::Settings);
    }

    #[test]
    fn update_all_count_excludes_held_and_modified() {
        let s = AppState::sample();
        let behind = s.listing.entries().filter(|e| e.has_update()).count();
        let unheld = s.listing.entries().filter(|e| e.has_update() && !e.held && !e.is_modified()).count();
        assert!(unheld < behind, "sample has held/modified updates");
        assert_eq!(s.update_all_count(), unheld);
        assert_eq!(s.update_all_count() + s.update_all_skipped(), behind);
    }

    fn line(a: &mut App, text: &str) {
        a.handle_event(RunnerEvent::Line { text: text.into(), stream: Stream::Stdout });
    }

    fn phases(a: &App) -> Vec<(String, Progress)> {
        let mut v: Vec<_> = a.state.progress.iter().map(|(n, c)| (n.clone(), c.phase.clone())).collect();
        v.sort_by(|x, y| x.0.cmp(&y.0));
        v
    }

    /// Synthetic Line events: Update all's bars go Queued -> Running -> result, one per result line,
    /// in the CLI's order; unrelated lines and names without a bar change nothing.
    #[test]
    fn update_all_bars_follow_the_result_lines() {
        let mut a = loaded();
        a.run_user_command(Command::Update { name: None });
        let c = a.next_to_start().expect("update starts");
        assert_eq!(c, Command::Update { name: None });
        let set: Vec<String> = a.state.listing.entries().filter(|e| e.updatable()).map(|e| e.name.clone()).collect();
        assert!(set.len() >= 2, "the sample has several updatable programs: {set:?}");
        let mut keys: Vec<_> = a.state.progress.keys().cloned().collect();
        keys.sort();
        let mut sorted = set.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "exactly the Update all set (update_all_count)");
        assert_eq!(a.state.progress.len(), a.state.update_all_count());
        assert_eq!(a.state.progress[&set[0]].phase, Progress::Running);
        assert!(set[1..].iter().all(|n| a.state.progress[n].phase == Progress::Queued));
        assert!(a.state.progress_running());

        line(&mut a, "Checking for updates ...");
        line(&mut a, "held-one: held (ritz-get unhold held-one)");
        assert_eq!(a.state.progress[&set[0]].phase, Progress::Running, "nothing of ours yet");

        line(&mut a, &format!("{}: updated aaa1111 -> bbb2222", set[0]));
        assert_eq!(a.state.progress[&set[0]].label(), "Updated");
        assert_eq!(a.state.progress[&set[1]].phase, Progress::Running, "the next one runs");
        // A later program finishing first (up to date prints in the check phase): the runner stays put.
        if let Some(last) = set.get(2) {
            line(&mut a, &format!("{last}: up to date"));
            assert_eq!(a.state.progress[last].label(), "Up to date");
            assert_eq!(a.state.progress[&set[1]].phase, Progress::Running);
        }
        line(&mut a, &format!("{}: FAILED (install.sh exit 1)", set[1]));
        assert_eq!(a.state.progress[&set[1]].phase, Progress::Failed);
        a.handle_event(fin(Command::Update { name: None }, ExitKind::Failed, Captured::default()));
        // Whatever never got a line (exit != 0, Update all): no bar.
        assert!(a.state.progress.values().all(|c| c.is_final() && c.ended.is_some()));
        assert!(a.state.progress.contains_key(&set[0]) && a.state.progress.contains_key(&set[1]));
        assert!(a.state.loading, "the refresh follows; the bars wait for it");
    }

    /// Start Update all on the loaded sample; the updatable names in listing (= CLI) order.
    fn update_all_started() -> (App, Vec<String>) {
        let mut a = loaded();
        // the sample has two updatable programs; two more fall behind
        for e in a.state.listing.sections.iter_mut().flat_map(|s| s.entries.iter_mut()) {
            if matches!(e.name.as_str(), "SteamAlt" | "ccorral") {
                e.status = Status::Behind;
                e.behind = Some(2);
                e.latest = Version::parse("2026-10-23 e55f6a7");
            }
        }
        a.run_user_command(Command::Update { name: None });
        a.next_to_start().expect("update starts");
        let set: Vec<String> = a.state.listing.entries().filter(|e| e.updatable()).map(|e| e.name.clone()).collect();
        assert!(set.len() >= 3, "the sample has at least three updatable programs: {set:?}");
        (a, set)
    }

    fn title(a: &App) -> String {
        a.state.run_all_title()
    }

    fn visible_log(a: &App) -> Vec<String> {
        a.state.log.iter().map(|l| l.text.clone()).collect()
    }

    /// Update all with `#start` / `#progress`: phase 1 results are final and not counted, the running
    /// card is exactly the latest `#start`, the header is honest until ritz-get said how many.
    #[test]
    fn markers_drive_update_all() {
        let (mut a, set) = update_all_started();
        let m = set.len() - 1; // the first one is settled in phase 1 (up to date): apply list = the rest
        assert_eq!(title(&a), "Checking for updates\u{2026}", "phase 1: nothing is known yet");
        line(&mut a, "Checking for updates ...");
        line(&mut a, &format!("{}: up to date", set[0]));
        line(&mut a, "bar: held (ritz-get unhold bar)");
        assert_eq!(a.state.progress[&set[0]].label(), "Up to date", "final at once");
        assert_eq!(title(&a), "Checking for updates\u{2026}", "an up-to-date line is not an update");
        assert!(!a.state.run_counted());

        line(&mut a, &format!("#start {}", set[1]));
        assert_eq!(a.state.progress[&set[1]].phase, Progress::Running);
        assert!(set[2..].iter().all(|n| a.state.progress[n].phase == Progress::Queued));
        assert_eq!(a.state.progress[&set[0]].label(), "Up to date", "phase-1 result untouched");
        assert_eq!(title(&a), format!("Updating {}\u{2026}", a.state.display_of(&set[1])), "M is not known before the first #progress");
        assert_eq!(a.state.reopen_label(), None, "the dialog is showing");

        line(&mut a, &format!("{}: updated aaa1111 -> bbb2222", set[1]));
        assert_eq!(a.state.progress[&set[1]].label(), "Updated");
        assert_eq!(a.state.progress[&set[2]].phase, Progress::Queued, "no inference once #start was seen");
        line(&mut a, &format!("#progress {} 1/{m}", set[1]));
        assert_eq!(a.state.run_counts(), (1, m), "the apply list only: the phase-1 program is not in M");
        assert_eq!(title(&a), format!("Updating 2 of {m}\u{2026}"));

        line(&mut a, &format!("#start {}", set[2]));
        assert_eq!(a.state.progress[&set[2]].phase, Progress::Running);
        line(&mut a, &format!("{}: FAILED (install.sh exit 1)", set[2]));
        line(&mut a, &format!("#progress {} 2/{m}", set[2])); // also on failure
        assert_eq!(a.state.progress[&set[2]].phase, Progress::Failed);
        assert_eq!(a.state.run_counts(), (2, m));
        a.state.modal = None; // "Run in background"
        assert_eq!(a.state.reopen_label(), Some(format!("Updating {} of {m}\u{2026}", (3).min(m))));

        // the machine lines are not in the log (so not in Copy log)
        assert!(visible_log(&a).iter().all(|l| !l.starts_with('#')), "{:?}", visible_log(&a));
        assert!(visible_log(&a).iter().any(|l| l.contains("up to date")));
    }

    /// ritz-get updating itself first: `1/1`, then (after the re-exec) `1/M`: one run, counts add up,
    /// the header never goes backwards.
    #[test]
    fn self_update_segments_add_up() {
        let (mut a, set) = update_all_started();
        let m = 2; // the second part: two programs
        line(&mut a, "#start ritz-get");
        line(&mut a, "ritz-get: updated aaa1111 -> bbb2222");
        line(&mut a, "#progress ritz-get 1/1");
        assert_eq!(a.state.run_counts(), (1, 1));
        let mut heads = vec![title(&a)];
        // re-exec: phase 1 again (results of the same programs are idempotent), then the apply list
        line(&mut a, "bar: held (ritz-get unhold bar)");
        line(&mut a, &format!("#start {}", set[0]));
        heads.push(title(&a));
        assert_eq!(a.state.marks.base, 1, "a new segment");
        assert_eq!(a.state.progress[&set[0]].phase, Progress::Running);
        line(&mut a, &format!("{}: updated aaa1111 -> bbb2222", set[0]));
        line(&mut a, &format!("#progress {} 1/{m}", set[0]));
        heads.push(title(&a));
        assert_eq!(a.state.run_counts(), (2, 1 + m), "1 + 1 of 1 + 2");
        line(&mut a, &format!("#start {}", set[1]));
        line(&mut a, &format!("{}: updated aaa1111 -> bbb2222", set[1]));
        line(&mut a, &format!("#progress {} 2/{m}", set[1]));
        heads.push(title(&a));
        assert_eq!(a.state.run_counts(), (3, 3));
        assert_eq!(
            heads,
            [
                "Updating 1 of 1\u{2026}".to_owned(),
                format!("Updating {}\u{2026}", a.state.display_of(&set[0])),
                "Updating 3 of 3\u{2026}".to_owned(), // ritz-get, set[0] done; set[1] is the third of three
                "Updating 3 of 3\u{2026}".to_owned(),
            ]
        );
    }

    /// An older ritz-get (no markers): the first update line proves it, then the old inference runs.
    #[test]
    fn without_markers_the_inference_stays() {
        let (mut a, set) = update_all_started();
        assert_eq!(a.state.progress[&set[0]].phase, Progress::Running, "today's guess");
        assert_eq!(title(&a), "Checking for updates\u{2026}");
        line(&mut a, &format!("{}: updated aaa1111 -> bbb2222", set[0]));
        assert!(a.state.marks.legacy && !a.state.marks.started);
        assert_eq!(a.state.progress[&set[1]].phase, Progress::Running, "the next one runs");
        assert_eq!(a.state.run_counts(), (1, set.len()));
        assert_eq!(title(&a), format!("Updating 2 of {}\u{2026}", set.len()));
        line(&mut a, &format!("{}: FAILED (install.sh exit 1)", set[1]));
        assert_eq!(a.state.progress[&set[2]].phase, Progress::Running);
        assert_eq!(a.state.run_counts(), (2, set.len()));
        // a failed fetch is a phase-1 line: it proves nothing
        let (mut b, set) = update_all_started();
        line(&mut b, &format!("{}: FAILED (fetch failed)", set[0]));
        assert!(!b.state.marks.legacy);
    }

    /// Markers on a single command change nothing but the counters; other `#` lines are ignored and hidden.
    #[test]
    fn single_install_with_markers() {
        let mut a = loaded();
        a.run_user_command(Command::Install { name: "ritz".into(), branch: None });
        a.next_to_start().expect("install starts");
        line(&mut a, "#start ritz");
        line(&mut a, "Cloning ritz (main) ...");
        line(&mut a, "#future thing 1");
        line(&mut a, "# a comment of install.sh");
        line(&mut a, "ritz: FAILED (needs: cmake)");
        line(&mut a, "#progress ritz 1/1");
        assert_eq!(a.state.progress["ritz"].phase, Progress::Failed);
        assert_eq!(a.state.marks.total(), Some(1));
        assert_eq!(visible_log(&a)[1..], ["Cloning ritz (main) ...", "# a comment of install.sh", "ritz: FAILED (needs: cmake)"]);
        // two names (CLI): the second has no card here, nothing breaks
        let mut a = loaded();
        a.run_user_command(Command::Install { name: "ritz".into(), branch: None });
        a.next_to_start().expect("install starts");
        for l in ["#start ritz", "ritz: installed", "#progress ritz 1/2", "#start other", "other: installed", "#progress other 2/2"] {
            line(&mut a, l);
        }
        assert_eq!(a.state.progress.len(), 1);
        assert_eq!(a.state.progress["ritz"].phase, Progress::Running, "its own result line is `Installed`-style: exit decides");
        assert_eq!(a.state.run_counts(), (2, 2));
    }

    /// A marker on stderr is just text.
    #[test]
    fn stderr_hash_lines_are_text() {
        let (mut a, set) = update_all_started();
        a.handle_event(RunnerEvent::Line { text: format!("#start {}", set[1]), stream: Stream::Stderr });
        assert!(!a.state.marks.started);
        assert!(visible_log(&a).iter().any(|l| l.starts_with("#start")));
    }

    #[test]
    fn single_command_exit_decides_without_a_result_line() {
        for (exit, want) in [(ExitKind::Ok, Progress::Done), (ExitKind::Failed, Progress::Failed), (ExitKind::Deps, Progress::Failed)] {
            let mut a = loaded();
            let cmd = Command::Switch { name: "ts6tray".into(), branch: "dev".into() };
            a.run_user_command(cmd.clone());
            a.next_to_start();
            assert_eq!(phases(&a), [("ts6tray".to_owned(), Progress::Running)]);
            assert_eq!(a.state.progress["ts6tray"].verb, Verb::Switch);
            line(&mut a, "Switching ts6tray to dev ...");
            assert_eq!(a.state.progress["ts6tray"].phase, Progress::Running, "not a result line");
            a.handle_event(fin(cmd, exit, Captured::default()));
            assert_eq!(a.state.progress["ts6tray"].phase, want, "{exit:?}");
        }
        for exit in [ExitKind::Busy, ExitKind::SpawnFailed] {
            let mut a = loaded();
            let cmd = Command::Reset { name: "geofinder".into() };
            a.run_user_command(cmd.clone());
            a.next_to_start();
            a.handle_event(fin(cmd, exit, Captured::default()));
            assert!(a.state.progress.is_empty(), "{exit:?}: nothing happened");
        }
    }

    /// Other commands (list, hold, remove, ...) neither start nor disturb bars.
    #[test]
    fn untracked_commands_leave_the_bars_alone() {
        let mut a = loaded();
        let cmd = Command::Update { name: Some("ts6tray".into()) };
        a.run_user_command(cmd.clone());
        a.next_to_start();
        a.handle_event(fin(cmd, ExitKind::Ok, Captured::default()));
        let before = phases(&a);
        assert_eq!(before, [("ts6tray".to_owned(), Progress::Done)]);
        // the refresh `list` starts and finishes
        let c = a.next_to_start().expect("refresh");
        assert!(matches!(c, Command::List { .. }));
        assert_eq!(phases(&a), before);
        a.handle_event(fin(c, ExitKind::Ok, out("#porcelain 1\n")));
        assert_eq!(phases(&a), before);
        a.run_user_command(Command::Hold { name: "ccorral".into() });
        a.next_to_start();
        assert_eq!(phases(&a), before);
        // Remove is not tracked either.
        assert!(!tracked(&Command::Remove { name: "x".into() }));
    }

    #[test]
    fn labels_and_tones() {
        let cp = |phase, verb| CardProgress { phase, verb, ended: None };
        let upd = Outcome::Updated { from: "a".into(), to: "b".into() };
        let cases = [
            (cp(Progress::Queued, Verb::Update), "Queued", Tone::Queued),
            (cp(Progress::Running, Verb::Update), "Updating\u{2026}", Tone::Busy),
            (cp(Progress::Running, Verb::Install), "Installing\u{2026}", Tone::Busy),
            (cp(Progress::Running, Verb::Repair), "Repairing\u{2026}", Tone::Busy),
            (cp(Progress::Done, Verb::Install), "Done", Tone::Good),
            (cp(Progress::Result(upd.clone()), Verb::Update), "Updated", Tone::Good),
            (cp(Progress::Result(upd), Verb::Reset), "Done", Tone::Good),
            (cp(Progress::Result(Outcome::UpToDate), Verb::Update), "Up to date", Tone::Even),
            (cp(Progress::Result(Outcome::SkippedLocalChanges), Verb::Update), "Skipped", Tone::Warn),
            (cp(Progress::Result(Outcome::SkippedNeedsNewer), Verb::Update), "Skipped", Tone::Warn),
            (cp(Progress::Result(Outcome::Held), Verb::Update), "Skipped", Tone::Warn),
            (cp(Progress::Failed, Verb::Install), "FAILED", Tone::Danger),
        ];
        for (c, label, tone) in cases {
            assert_eq!((c.label(), c.tone()), (label, tone), "{c:?}");
        }
    }

    /// Finished bars fade PROGRESS_FADE after the command ended, only once idle (refresh in).
    #[test]
    fn bars_fade_after_the_delay_when_idle() {
        let mut s = AppState::sample();
        s.begin_progress(&Command::Install { name: "ForzaTelemetryV3Launcher".into(), branch: None });
        assert_eq!(s.progress["ForzaTelemetryV3Launcher"].verb, Verb::Install);
        let t0 = Instant::now();
        s.busy = true;
        assert_eq!(s.expire_progress(t0 + PROGRESS_FADE * 9), None, "busy: never");
        s.finish_progress(ExitKind::Ok, t0);
        s.loading = true;
        s.busy = false;
        assert_eq!(s.expire_progress(t0 + PROGRESS_FADE * 9), None, "refresh pending");
        s.loading = false;
        assert!(s.expire_progress(t0 + PROGRESS_FADE / 2).is_some_and(|d| d <= PROGRESS_FADE / 2 + Duration::from_millis(1)));
        assert!(!s.progress.is_empty());
        assert_eq!(s.expire_progress(t0 + PROGRESS_FADE), None);
        assert!(s.progress.is_empty());
        // install of an installed program is a repair
        s.begin_progress(&Command::Install { name: "ts6tray".into(), branch: None });
        assert_eq!(s.progress["ts6tray"].verb, Verb::Repair);
    }

    /// Esc / backdrop on the progress dialog: while it runs = Run in background (the run stays and can
    /// be shown again); after it = dismissed. The log dialog just closes.
    #[test]
    fn closing_the_progress_dialog_backgrounds_or_dismisses() {
        let mut a = loaded();
        let cmd = Command::Update { name: Some("ts6tray".into()) };
        a.queue.push_back(cmd.clone());
        a.next_to_start();
        assert_eq!(a.state.modal, Some(Modal::Run));
        assert_eq!(a.state.reopen_label(), None);
        a.state.apply_ui_action(Action::CloseModal);
        assert!(a.state.modal.is_none() && a.state.run.as_ref().is_some_and(Run::running));
        assert_eq!(a.state.reopen_label().as_deref(), Some("Updating TeamSpeak 6 Tray\u{2026}"));
        a.state.apply_ui_action(Action::ShowModal(Modal::Run));
        assert_eq!(a.state.modal, Some(Modal::Run));
        a.handle_event(fin(cmd, ExitKind::Failed, Captured::default()));
        assert!(matches!(a.state.run.as_ref().map(|r| &r.phase), Some(RunPhase::Failed { .. })));
        a.state.apply_ui_action(Action::CloseModal);
        assert!(a.state.modal.is_none() && a.state.run.is_none(), "after the end, close dismisses");
        // the log dialog
        a.state.apply_ui_action(Action::ShowModal(Modal::Log));
        assert_eq!(a.state.modal, Some(Modal::Log));
        a.state.apply_ui_action(Action::CloseModal);
        assert!(a.state.modal.is_none());
        // no run: nothing to show again
        a.state.apply_ui_action(Action::ShowModal(Modal::Run));
        assert!(a.state.modal.is_none());
    }

    /// The `startmenu` shot hook shows the post-install situation behind the modal.
    #[test]
    fn startmenu_shot_hook_marks_the_entry_installed() {
        let mut s = AppState::sample();
        s.apply_shot_hook("startmenu=Software/ForzaTelemetryV3Launcher");
        let e = s.listing.find("Software/ForzaTelemetryV3Launcher").unwrap();
        assert!(e.is_installed() && e.status == Status::Current && e.installed == e.latest && e.shortcut == ShortcutState::Off);
        assert_eq!(s.modal, Some(Modal::Run));
        assert_eq!(s.run.as_ref().map(|r| r.phase.clone()), Some(RunPhase::Done { menu: true }));
    }

    /// The `progress` shot hook: one Running, a Failed, results, the rest Queued; frozen (busy, no clock).
    #[test]
    fn progress_shot_hook_is_frozen_mid_run() {
        let mut s = AppState::sample();
        s.apply_shot_hook("progress");
        assert!(s.busy && s.progress_running());
        let count = |f: &dyn Fn(&CardProgress) -> bool| s.progress.values().filter(|c| f(c)).count();
        assert_eq!(count(&|c| c.phase == Progress::Running), 1);
        assert_eq!(count(&|c| c.phase == Progress::Failed), 1);
        assert!(count(&|c| c.phase == Progress::Queued) >= 1);
        assert!(s.progress.values().all(|c| c.ended.is_none()));
        assert_eq!(s.expire_progress(Instant::now() + PROGRESS_FADE * 9), None);
        assert!(!s.progress.is_empty());
    }

    #[test]
    fn icons_refresh_after_update_install_not_remove() {
        let a = loaded();
        let dir = a.state.dir.clone().unwrap();
        let ic = |n: &str| dir.join(n).join("icon.png");
        let upd = |n: &str| ProgramResult { name: n.into(), outcome: Outcome::Updated { from: "a".into(), to: "b".into() } };
        let skip = ProgramResult { name: "bar".into(), outcome: Outcome::Failed { reason: "x".into() } };
        let cap = Captured { results: vec![upd("foo"), skip, upd("baz")], ..Captured::default() };
        let up = Command::Update { name: None };
        assert_eq!(a.icons_to_refresh(&up, ExitKind::Ok, &cap), vec![ic("foo"), ic("baz")]);
        let one = Captured { results: vec![upd("foo")], ..Captured::default() };
        assert_eq!(a.icons_to_refresh(&Command::Update { name: Some("foo".into()) }, ExitKind::Ok, &one), vec![ic("foo")]);
        let none = Captured::default();
        assert_eq!(a.icons_to_refresh(&Command::Install { name: "foo".into(), branch: None }, ExitKind::Ok, &none), vec![ic("foo")]);
        assert_eq!(a.icons_to_refresh(&Command::Reset { name: "foo".into() }, ExitKind::Ok, &none), vec![ic("foo")]);
        assert_eq!(a.icons_to_refresh(&Command::Switch { name: "foo".into(), branch: "b".into() }, ExitKind::Ok, &none), vec![ic("foo")]);
        assert!(a.icons_to_refresh(&Command::Remove { name: "foo".into() }, ExitKind::Ok, &cap).is_empty());
        assert!(a.icons_to_refresh(&up, ExitKind::Failed, &cap).is_empty());
        assert!(a.icons_to_refresh(&Command::Install { name: "foo".into(), branch: None }, ExitKind::Failed, &none).is_empty());
    }

    #[test]
    fn self_update_sets_restart() {
        let mut a = loaded();
        assert!(!a.state.restart);
        let upd = |name: &str| ProgramResult { name: name.into(), outcome: Outcome::Updated { from: "a".into(), to: "b".into() } };
        let cap = Captured { results: vec![upd("foo")], summary: Some(RunSummary { updated: 1, skipped: 0, failed: 0 }), ..Captured::default() };
        a.handle_event(fin(Command::Update { name: None }, ExitKind::Ok, cap));
        assert!(!a.state.restart);
        let cap = Captured { results: vec![upd("foo"), upd("ritz-get-gui")], ..Captured::default() };
        a.handle_event(fin(Command::Update { name: None }, ExitKind::Ok, cap));
        assert!(a.state.restart);
        let mut b = loaded();
        let cap = Captured { results: vec![upd("ritz-get-gui")], ..Captured::default() };
        b.handle_event(fin(Command::Update { name: Some("ritz-get-gui".into()) }, ExitKind::Ok, cap));
        assert!(b.state.restart);
    }

    #[test]
    fn ok_run_clears_badge_and_refreshes() {
        let mut a = loaded();
        let (name, _) = some_name(&a);
        a.state.badges.insert(name.clone(), Badge::SetupNeeded);
        a.handle_event(fin(Command::Install { name: name.clone(), branch: None }, ExitKind::Ok, Captured::default()));
        assert!(!a.state.badges.contains_key(&name));
        assert_eq!(a.queue.front(), Some(&list(false)));
        // `open` does not refresh.
        let mut b = loaded();
        b.handle_event(fin(Command::Open { name }, ExitKind::Ok, Captured::default()));
        assert!(b.queue.is_empty());
    }

    #[test]
    fn dispatch_queues_commands_and_gates_mutations() {
        let ctx = egui::Context::default();
        let mut a = loaded();
        let (name, _) = some_name(&a);
        a.dispatch(&ctx, Action::Hold { name: name.clone() });
        assert_eq!(a.queue.back(), Some(&Command::Hold { name: name.clone() }));
        // Start it: while it runs, further mutations are refused, queries are queued.
        let c = a.next_to_start().unwrap();
        assert_eq!(c, Command::Hold { name: name.clone() });
        assert_eq!(a.state.log.last().unwrap().text, format!("$ ritz-get hold {name}"));
        a.dispatch(&ctx, Action::Remove { name: name.clone() });
        assert!(a.queue.is_empty());
        a.dispatch(&ctx, Action::Branches { name: name.clone() });
        assert_eq!(a.queue.len(), 1);
        // Queries leave no `$` line.
        a.handle_event(fin(c, ExitKind::Ok, Captured::default()));
        a.queue.clear();
        a.dispatch(&ctx, Action::CheckForUpdates);
        let n = a.state.log.len();
        assert_eq!(a.next_to_start(), Some(list(false)));
        assert_eq!(a.state.log.len(), n);
    }

    #[test]
    fn branches_are_stored_by_name() {
        let mut a = loaded();
        a.handle_event(fin(Command::Branches { name: "foo".into() }, ExitKind::Ok, out("* main\n  dev\n")));
        assert_eq!(a.state.branches["foo"], vec![("main".to_owned(), true), ("dev".to_owned(), false)]);
    }

    #[test]
    fn set_dir_requeries_then_lists() {
        let ctx = egui::Context::default();
        let mut a = loaded();
        a.dispatch(&ctx, Action::SetDir("/new".into()));
        let c = a.next_to_start().unwrap();
        assert_eq!(c, Command::Dir { path: Some("/new".into()) });
        a.handle_event(fin(c, ExitKind::Ok, Captured::default()));
        assert_eq!(a.queue.iter().cloned().collect::<Vec<_>>(), vec![Command::Dir { path: None }, list(false)]);
        let mut b = loaded();
        b.dispatch(&ctx, Action::SetShortcutsDir("/s".into()));
        let c = b.next_to_start().unwrap();
        b.handle_event(fin(c, ExitKind::Ok, Captured::default()));
        assert_eq!(b.queue.front(), Some(&Command::Shortcuts { path: None }));
    }

    #[test]
    fn streamed_lines_are_classified() {
        assert_eq!(classify("foo: FAILED (fetch failed)"), LogKind::Err);
        assert_eq!(classify("FAILED \u{2014} your installed version is unchanged and still works"), LogKind::Err);
        assert_eq!(classify("foo: updated 2026-10-01 aaa1111 -> 2026-10-14 bbb2222"), LogKind::Ok);
        assert_eq!(classify("foo: up to date"), LogKind::Info);
        assert_eq!(classify("busy: another ritz-get is running"), LogKind::Err);
        assert_eq!(classify("needs root: foo"), LogKind::Note);
        assert_eq!(classify("Additional steps required:"), LogKind::Note);
        let mut a = app();
        a.handle_event(RunnerEvent::Line { text: "foo: updated a -> b\n".into(), stream: Stream::Stdout });
        a.handle_event(RunnerEvent::Line { text: "".into(), stream: Stream::Stdout });
        assert_eq!(a.state.log.len(), 1);
        assert_eq!(a.state.log[0].kind, LogKind::Ok);
    }

    #[test]
    fn clock_and_log_cap() {
        let t = clock_now();
        assert_eq!(t.len(), 8);
        assert!(t.as_bytes()[2] == b':' && t.as_bytes()[5] == b':');
        let mut s = AppState::default();
        for i in 0..LOG_CAP + 10 {
            s.push_log(LogKind::Info, format!("{i}"));
        }
        assert_eq!(s.log.len(), LOG_CAP);
        assert_eq!(s.log.last().unwrap().text, format!("{}", LOG_CAP + 9));
    }

    #[test]
    fn sample_counts_and_filters() {
        let s = AppState::sample();
        let c = s.counts();
        assert_eq!(c.all, s.listing.entries().count());
        assert!(c.updates >= 3 && c.not_installed >= 3);
        // yt-dlp-gui is held, geofinder is modified: neither is counted by Update all.
        assert!(s.update_all_count() < c.updates);
        let mut s = AppState::sample();
        s.filter = Filter::Updates;
        assert!(s.visible_sections().iter().all(|(_, v)| v.iter().all(|e| e.has_update())));
        s.filter = Filter::All;
        s.search = "forza".into();
        assert_eq!(s.visible_sections().iter().map(|(_, v)| v.len()).sum::<usize>(), 1);
    }

    #[test]
    fn unreachable_entries_are_hidden_and_uncounted() {
        let mut s = AppState::sample();
        let before = s.counts().all;
        let e = s.listing.sections[0].entries[0].clone();
        let mut u = e.clone();
        u.name = "ghost".into();
        u.state = State::Unreachable;
        u.status = Status::None;
        s.listing.sections[0].entries.push(u);
        assert_eq!(s.counts().all, before);
        assert!(s.visible_sections().iter().all(|(_, v)| v.iter().all(|x| x.name != "ghost")));
    }

    #[test]
    fn sort_keeps_section_order_and_sorts_within() {
        let mut s = AppState::sample();
        let sections: Vec<&str> = s.listing.sections.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(sections.last(), Some(&Listing::UNLISTED));
        s.sort = Sort::Name;
        let vis = s.visible_sections();
        let vis_names: Vec<&str> = vis.iter().map(|(sec, _)| sec.name.as_str()).collect();
        assert!(vis_names.iter().zip(sections.iter().filter(|n| vis_names.contains(n))).all(|(a, b)| a == b));
        for (_, v) in &vis {
            let titles: Vec<String> = v.iter().map(|e| e.display().to_lowercase()).collect();
            let mut sorted = titles.clone();
            sorted.sort();
            assert_eq!(titles, sorted);
        }
    }
}
