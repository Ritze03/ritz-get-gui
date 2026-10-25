//! Shared types: the parsed `ritz-get list --porcelain` listing, the commands the GUI can run,
//! the `Action` enum the UI panes emit, and the small UI-state enums every pane reads.
//!
//! **Frozen contract (I1).** Written once by the scaffold step; the six parallel workers only
//! *read* this file. If a worker needs a change, report it to the lead instead of editing.
//!
//! Source of truth for the listing is `ritz-get/PORCELAIN.md` (porcelain 1). Mapping of the
//! `E` record (`E<TAB>` + 15 fields, plus the optional 17th column `shortcut`):
//!
//! | col | field     | here                                        |
//! |----:|-----------|---------------------------------------------|
//! | 2   | section   | `Entry::section`                            |
//! | 3   | name      | `Entry::name`                               |
//! | 4   | title     | `Entry::title` (`None` when == name)        |
//! | 5   | kind      | `Entry::kind`                               |
//! | 6   | source    | `Entry::source` (`-` -> `None`)             |
//! | 7   | state     | `Entry::state`                              |
//! | 8   | status    | `Entry::status`                             |
//! | 9   | held      | `Entry::held`                               |
//! | 10  | branch    | `Entry::branch` (`-` -> `None`)             |
//! | 11  | installed | `Entry::installed` (`-` -> `None`)          |
//! | 12  | latest    | `Entry::latest` (`-` -> `None`)             |
//! | 13  | behind    | `Entry::behind` (`-` -> `None`)             |
//! | 14  | dirty     | `Entry::dirty` (`-` -> `None`)              |
//! | 15  | ahead     | `Entry::ahead` (`-` -> `None`)              |
//! | 16  | desc      | `Entry::desc` (may be empty)                |
//! | 17  | shortcut  | `Entry::shortcut` (missing column -> `None`)|

use std::fmt;

// ---------------------------------------------------------------------------------------------
// Listing (porcelain)
// ---------------------------------------------------------------------------------------------

/// `E` field 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Program,
    Website,
}

impl Kind {
    /// `program` | `website`; anything else is `None` (the parser decides whether that is an error).
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "program" => Some(Kind::Program),
            "website" => Some(Kind::Website),
            _ => None,
        }
    }
}

/// `E` field 7. PORCELAIN vocabulary: `installed`, `available` (not installed, reachable),
/// `unreachable` (only with `--all`), `offline` (not installed, network unknown), `website`,
/// `unlisted` (clone not in the manifest). Anything else lands in `Other`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Installed,
    Available,
    Unreachable,
    Offline,
    Website,
    Unlisted,
    Other(String),
}

impl State {
    pub fn parse(s: &str) -> State {
        match s {
            "installed" => State::Installed,
            "available" => State::Available,
            "unreachable" => State::Unreachable,
            "offline" => State::Offline,
            "website" => State::Website,
            "unlisted" => State::Unlisted,
            other => State::Other(other.to_owned()),
        }
    }
}

/// `E` field 8. Only meaningful for `installed`/`unlisted` entries, else `-` ([`Status::None`]).
/// PORCELAIN vocabulary: `current`, `behind`, `modified` (wins over everything), `fetch-failed`,
/// `unknown` (installed but offline). `held` is NOT a status: it is the separate [`Entry::held`] flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// `-`
    None,
    Current,
    Behind,
    Modified,
    FetchFailed,
    Unknown,
    Other(String),
}

impl Status {
    pub fn parse(s: &str) -> Status {
        match s {
            "-" | "" => Status::None,
            "current" => Status::Current,
            "behind" => Status::Behind,
            "modified" => Status::Modified,
            "fetch-failed" => Status::FetchFailed,
            "unknown" => Status::Unknown,
            other => Status::Other(other.to_owned()),
        }
    }
}

/// `E` field 17 (D60): `on` | `off` | `-`. `None` = not installed or no `NAME.desktop`, and also
/// "the line had only 16 fields" (D4). The Create/Remove-shortcut action is shown only for On/Off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShortcutState {
    On,
    Off,
    #[default]
    None,
}

impl ShortcutState {
    pub fn parse(s: &str) -> ShortcutState {
        match s {
            "on" => ShortcutState::On,
            "off" => ShortcutState::Off,
            _ => ShortcutState::None,
        }
    }
}

/// `YYYY-MM-DD sha`, or (for `latest` of a not-installed program) a bare 7-char `sha` with no date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub date: Option<String>,
    pub sha: String,
}

impl Version {
    /// `"-"` / empty -> `None`; `"2026-10-14 3f9e21a"` -> date+sha; `"4be8a30"` -> bare sha.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        if s.is_empty() || s == "-" {
            return None;
        }
        match s.split_once(' ') {
            Some((date, sha)) => Some(Version { date: Some(date.to_owned()), sha: sha.trim().to_owned() }),
            None => Some(Version { date: None, sha: s.to_owned() }),
        }
    }
}

impl fmt::Display for Version {
    /// `"2026-10-14 3f9e21a"` or `"3f9e21a"`. Show in Geist Mono (D3).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.date {
            Some(d) => write!(f, "{d} {}", self.sha),
            None => write!(f, "{}", self.sha),
        }
    }
}

/// One `S` record: a manual setup step, attached to the `E` it follows (D21). `text` may contain
/// backtick code spans and URLs (the side panel makes URLs clickable).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupStep {
    pub text: String,
}

/// One `E` record (+ the `S` records after it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub section: String,
    pub name: String,
    /// `None` when the `title` column is empty or equal to `name`. Use [`Entry::display`].
    pub title: Option<String>,
    pub kind: Kind,
    /// `OWNER/REPO` or a URL; `None` for unlisted (`-`).
    pub source: Option<String>,
    pub state: State,
    pub status: Status,
    pub held: bool,
    /// Installed branch; for not-installed the manifest BRANCH. `None` for `-`.
    pub branch: Option<String>,
    pub installed: Option<Version>,
    /// Installed: upstream `date sha`. Not installed: bare sha. `None` for `-` (websites, offline).
    pub latest: Option<Version>,
    /// Commits behind upstream. `None` for `-`.
    pub behind: Option<u32>,
    /// Changed files in the working tree (count only; PORCELAIN gives no file names). `None` for `-`.
    pub dirty: Option<u32>,
    /// Local commits not on upstream. `None` for `-`.
    pub ahead: Option<u32>,
    /// Manifest description; may be empty.
    pub desc: String,
    pub shortcut: ShortcutState,
    /// `S` records, in order. Empty for unlisted entries.
    pub setup: Vec<SetupStep>,
}

impl Entry {
    /// Stable UI id, `"section/name"` (names can repeat across sections in the mockup, so the
    /// selection, collapse state and `Action::Select` use this, while commands use `name`).
    pub fn id(&self) -> String {
        format!("{}/{}", self.section, self.name)
    }

    /// D39: card/side-panel/sort title: `title` when set, otherwise `name`.
    pub fn display(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    pub fn is_website(&self) -> bool {
        self.kind == Kind::Website
    }

    /// A program with a local clone (`installed` or `unlisted`).
    pub fn is_installed(&self) -> bool {
        matches!(self.state, State::Installed | State::Unlisted)
    }

    pub fn is_unlisted(&self) -> bool {
        self.state == State::Unlisted
    }

    /// Upstream has newer commits: `status == behind`, or `behind > 0` (a `modified` program keeps
    /// its `behind` count even though `modified` wins the status column, so the mockup's
    /// "update available + modified" card still counts). See also [`Entry::updatable`].
    pub fn has_update(&self) -> bool {
        self.is_installed() && (self.status == Status::Behind || self.behind.unwrap_or(0) > 0)
    }

    pub fn is_modified(&self) -> bool {
        self.status == Status::Modified
    }

    /// Counted by "Update all (N)" (D19): behind, not held, not modified.
    pub fn updatable(&self) -> bool {
        self.has_update() && !self.held && !self.is_modified()
    }

    /// GitHub URL of the repo (`Open on GitHub`, `↗`): `https://github.com/OWNER/REPO` for
    /// `OWNER/REPO` sources, the URL itself for URL sources, `None` for unlisted/websites (D23).
    pub fn repo_url(&self) -> Option<String> {
        if self.is_website() {
            return None;
        }
        let src = self.source.as_deref()?;
        if src.starts_with("http://") || src.starts_with("https://") {
            Some(src.trim_end_matches('/').trim_end_matches(".git").to_owned())
        } else if src.contains('/') {
            Some(format!("https://github.com/{}", src.trim_end_matches(".git")))
        } else {
            None
        }
    }

    /// For a website: its URL (the `source` column). `None` for programs.
    pub fn website_url(&self) -> Option<&str> {
        if self.is_website() { self.source.as_deref() } else { None }
    }
}

/// A manifest section (or `Unlisted`, always last).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// A parsed `list --porcelain`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Listing {
    /// From `#porcelain N`. Always 1 for a successfully parsed listing.
    pub version: u32,
    /// From `#online 1|0`; `None` when the line was absent.
    pub online: Option<bool>,
    /// From `#hidden N`; 0 when absent.
    pub hidden: u32,
    /// Manifest order, `Unlisted` last.
    pub sections: Vec<Section>,
}

impl Listing {
    pub const UNLISTED: &'static str = "Unlisted";

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.sections.iter().flat_map(|s| s.entries.iter())
    }

    /// Find by [`Entry::id`] (`"section/name"`).
    pub fn find(&self, id: &str) -> Option<&Entry> {
        self.entries().find(|e| e.id() == id)
    }

    pub fn is_offline(&self) -> bool {
        self.online == Some(false)
    }
}

// ---------------------------------------------------------------------------------------------
// Commands (what the runner executes) and exit codes
// ---------------------------------------------------------------------------------------------

/// One `ritz-get` invocation. The runner (I3) executes exactly one at a time. `args()` is the
/// single source of truth for argv (D2, D48); the runner adds the env (`RITZ_GET_GUI=1`, and
/// `RITZ_GET_SUDO=pkexec` when [`Command::uses_sudo`]) and sets stdin to `/dev/null`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `list [--offline] --porcelain [--all]`
    List { offline: bool, all: bool },
    /// `install NAME [--branch B]` (repair when installed)
    Install { name: String, branch: Option<String> },
    /// `update [NAME] --yes`
    Update { name: Option<String> },
    /// `remove NAME`
    Remove { name: String },
    /// `switch NAME BRANCH`
    Switch { name: String, branch: String },
    /// `reset NAME --yes`
    Reset { name: String },
    /// `hold NAME`
    Hold { name: String },
    /// `unhold NAME`
    Unhold { name: String },
    /// `open NAME` (websites)
    Open { name: String },
    /// `shortcut NAME [--remove]`
    Shortcut { name: String, remove: bool },
    /// `shortcuts [PATH]`
    Shortcuts { path: Option<String> },
    /// `dir [PATH]`
    Dir { path: Option<String> },
    /// `branches NAME`
    Branches { name: String },
}

impl Command {
    /// argv after the binary name. Order is deliberate: the startup calls read
    /// `list --offline --porcelain` and `list --porcelain` (plan "Done when").
    pub fn args(&self) -> Vec<String> {
        let s = |x: &str| x.to_owned();
        match self {
            Command::List { offline, all } => {
                let mut a = vec![s("list")];
                if *offline {
                    a.push(s("--offline"));
                }
                a.push(s("--porcelain"));
                if *all {
                    a.push(s("--all"));
                }
                a
            }
            Command::Install { name, branch } => {
                let mut a = vec![s("install"), name.clone()];
                if let Some(b) = branch {
                    a.push(s("--branch"));
                    a.push(b.clone());
                }
                a
            }
            Command::Update { name } => {
                let mut a = vec![s("update")];
                if let Some(n) = name {
                    a.push(n.clone());
                }
                a.push(s("--yes"));
                a
            }
            Command::Remove { name } => vec![s("remove"), name.clone()],
            Command::Switch { name, branch } => vec![s("switch"), name.clone(), branch.clone()],
            Command::Reset { name } => vec![s("reset"), name.clone(), s("--yes")],
            Command::Hold { name } => vec![s("hold"), name.clone()],
            Command::Unhold { name } => vec![s("unhold"), name.clone()],
            Command::Open { name } => vec![s("open"), name.clone()],
            Command::Shortcut { name, remove } => {
                let mut a = vec![s("shortcut"), name.clone()];
                if *remove {
                    a.push(s("--remove"));
                }
                a
            }
            Command::Shortcuts { path } => {
                let mut a = vec![s("shortcuts")];
                a.extend(path.clone());
                a
            }
            Command::Dir { path } => {
                let mut a = vec![s("dir")];
                if let Some(p) = path {
                    // PORCELAIN: "Always pass `--yes` to `update`, `reset` and `dir PATH`".
                    a.push(p.clone());
                    a.push(s("--yes"));
                }
                a
            }
            Command::Branches { name } => vec![s("branches"), name.clone()],
        }
    }

    /// D48: only install, remove, switch and reset get `RITZ_GET_SUDO=pkexec`.
    pub fn uses_sudo(&self) -> bool {
        matches!(
            self,
            Command::Install { .. } | Command::Remove { .. } | Command::Switch { .. } | Command::Reset { .. }
        )
    }

    /// Read-only calls whose stdout is data, not log text: `list`, `branches`, and `dir`/`shortcuts`
    /// without a PATH. The runner returns their stdout in `Captured::stdout` and emits no
    /// `RunnerEvent::Line` for it (stderr is still streamed); the app does not log a `$ ritz-get ...` line.
    pub fn is_query(&self) -> bool {
        matches!(
            self,
            Command::List { .. }
                | Command::Branches { .. }
                | Command::Dir { path: None }
                | Command::Shortcuts { path: None }
        )
    }

    /// The entry this command targets, if any (used for session badges).
    pub fn target(&self) -> Option<&str> {
        match self {
            Command::Install { name, .. }
            | Command::Remove { name }
            | Command::Switch { name, .. }
            | Command::Reset { name }
            | Command::Hold { name }
            | Command::Unhold { name }
            | Command::Open { name }
            | Command::Shortcut { name, .. }
            | Command::Branches { name } => Some(name),
            Command::Update { name } => name.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for Command {
    /// `ritz-get install foo --branch dev`: the `$ ...` line shown in the log / running-command header.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ritz-get {}", self.args().join(" "))
    }
}

/// Exit code of one command (PORCELAIN.md "Exit codes", D48).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitKind {
    /// 0, including held / modified / aborted / busy-from-cron.
    Ok,
    /// 1: something failed.
    Failed,
    /// 2: usage error.
    Usage,
    /// 3: a program needs tools that are missing; a `deps:` line (or raw `missing:` lines) follows.
    Deps,
    /// 4: a program needs root setup (`needs root:`); run `ritz-get install NAME` in a terminal.
    NeedsRoot,
    /// 5: another ritz-get holds the lock.
    Busy,
    /// The binary could not be spawned (not found / not executable). Drives the "ritz-get not found" banner.
    SpawnFailed,
    /// Any other code (also killed by a signal -> `Other(-1)`).
    Other(i32),
}

impl ExitKind {
    /// `None` = terminated by a signal.
    pub fn from_code(code: Option<i32>) -> ExitKind {
        match code {
            Some(0) => ExitKind::Ok,
            Some(1) => ExitKind::Failed,
            Some(2) => ExitKind::Usage,
            Some(3) => ExitKind::Deps,
            Some(4) => ExitKind::NeedsRoot,
            Some(5) => ExitKind::Busy,
            Some(n) => ExitKind::Other(n),
            None => ExitKind::Other(-1),
        }
    }

    pub fn is_ok(self) -> bool {
        self == ExitKind::Ok
    }
}

// ---------------------------------------------------------------------------------------------
// UI state enums
// ---------------------------------------------------------------------------------------------

/// Which page fills the body (the page pill in the top bar reads "Programs" / "Settings").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Main,
    Settings,
}

/// Top-bar filter tabs, in tab order (mockup: All, Installed, Updates, Not installed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Filter {
    #[default]
    All,
    Installed,
    Updates,
    NotInstalled,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Installed, Filter::Updates, Filter::NotInstalled];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Updates => "Updates",
            Filter::NotInstalled => "Not installed",
        }
    }

    /// Mockup semantics: Installed = installed/unlisted programs; Updates = status behind;
    /// Not installed = programs (not websites) that are not installed; All = everything.
    pub fn matches(self, e: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Installed => e.is_installed(),
            Filter::Updates => e.has_update(),
            Filter::NotInstalled => !e.is_website() && !e.is_installed(),
        }
    }
}

/// Sort dropdown (D43). Sorts within each section; sections keep manifest order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Sort {
    #[default]
    Default,
    Name,
    UpdatesFirst,
    InstalledFirst,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Default, Sort::Name, Sort::UpdatesFirst, Sort::InstalledFirst];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Default => "Default",
            Sort::Name => "Name",
            Sort::UpdatesFirst => "Updates first",
            Sort::InstalledFirst => "Installed first",
        }
    }

    /// Key used in `~/.config/ritz-get-gui/settings`.
    pub fn key(self) -> &'static str {
        match self {
            Sort::Default => "default",
            Sort::Name => "name",
            Sort::UpdatesFirst => "updates",
            Sort::InstalledFirst => "installed",
        }
    }

    pub fn from_key(s: &str) -> Sort {
        Sort::ALL.into_iter().find(|x| x.key() == s).unwrap_or_default()
    }
}

/// Per-card badge from the *session* (not from porcelain): set by the app after a run (D58).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Badge {
    /// Exit 3: tools reported by `missing:` / `FAILED (needs: a, b)` -> `NEEDS <tool>` pill (DANGER).
    Needs(Vec<String>),
    /// Exit 4: `needs root: ...` -> `SETUP NEEDED` pill (WARN) + "run `ritz-get install NAME` in a terminal".
    SetupNeeded,
}

/// A modal dialog (egui `Modal`, Esc / backdrop click cancels). Drawn by `ui::modals`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Modal {
    /// D22/D32/D61: Cancel is the default; "Open folder" first; red "Discard and reset". `id` = `Entry::id`.
    Reset { id: String },
    /// The install / update / repair / switch / reset / remove (or Update all) progress dialog; its
    /// content is [`crate::app::AppState::run`]. While the command runs, Esc / backdrop = "Run in
    /// background" (the dialog closes, the run goes on); after it, they close it for good.
    /// Replaces the old Deps (D54/D58) and StartMenu (D47) dialogs: exit 3 shows "Missing tools" and
    /// a fresh install asks "Add it to the start menu?" inside this dialog.
    Run,
    /// The whole log (status bar "Log" / "last update failed (see log)").
    Log,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    /// plain output line
    Info,
    /// `$ ritz-get ...` (TEXT_DIM)
    Cmd,
    /// success (GOOD)
    Ok,
    /// error / FAILED (DANGER)
    Err,
    /// note, e.g. "Additional steps required:" (ACCENT)
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// Local time `HH:MM:SS`.
    pub time: String,
    pub kind: LogKind,
    pub text: String,
}

/// `~/.cache/ritz-get/last-run`: `<ISO time> N updated, N skipped, N failed` + optional `failed: NAME NAME`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LastRun {
    pub time: String,
    pub updated: u32,
    pub skipped: u32,
    pub failed: u32,
    pub failed_names: Vec<String>,
}

impl LastRun {
    pub fn has_failures(&self) -> bool {
        self.failed > 0
    }
}

// ---------------------------------------------------------------------------------------------
// Action: everything the UI can ask for
// ---------------------------------------------------------------------------------------------

/// What a UI pane wants done. Panes get `&AppState` (read-only) and push into
/// `&mut Vec<Action>`; `App::update` applies them after drawing (so a pane never mutates state
/// mid-frame). `name` is always the ritz-get NAME; `Select`/`ToggleGroup`/`Modal` use
/// [`Entry::id`] (`"section/name"`) / the section name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    // ---- ritz-get commands (see `Action::to_command`) ----
    /// Not-installed program. `branch: None` = manifest default.
    Install { name: String, branch: Option<String> },
    /// Installed program: re-runs install.sh (`install NAME`).
    Repair { name: String },
    /// `None` = "Update all".
    Update { name: Option<String> },
    Remove { name: String },
    /// Installed program, change branch.
    Switch { name: String, branch: String },
    /// Run the reset (after the Reset modal's "Discard and reset").
    Reset { name: String },
    /// Auto-update off.
    Hold { name: String },
    /// Auto-update on.
    Unhold { name: String },
    /// Website: `ritz-get open NAME`.
    Open { name: String },
    /// `remove: false` = create (`shortcut NAME`), `true` = `shortcut NAME --remove`.
    Shortcut { name: String, remove: bool },
    /// `ritz-get dir PATH` (Settings -> Folders -> Change...).
    SetDir(String),
    /// `ritz-get shortcuts PATH`.
    SetShortcutsDir(String),
    /// Re-run `list --porcelain` (top bar "Check for updates").
    CheckForUpdates,
    /// Fetch the branch list for the side-panel ComboBox (`branches NAME`); result lands in `AppState::branches`.
    Branches { name: String },

    // ---- not ritz-get ----
    /// `xdg-open URL` (Report issue, Open on GitHub, README, setup-step links, status-bar report link).
    OpenUrl(String),
    /// `xdg-open PATH` (Open folder; callers build `<dir>/<name>`).
    OpenFolder(String),
    CopyToClipboard(String),
    /// Copy the whole log to the clipboard.
    CopyLog,
    /// Re-exec `~/.local/bin/ritz-get-gui` (restart bar, D25).
    Restart,
    /// Re-run the startup `list` (the "ritz-get not found" banner's Retry).
    Retry,

    // ---- UI state ----
    /// Select the entry with this `Entry::id`.
    Select(String),
    /// Collapse/expand the section with this name.
    ToggleGroup(String),
    SetFilter(Filter),
    SetSearch(String),
    SetSort(Sort),
    /// Settings > Touch mode: drag-to-scroll on or off.
    SetTouchMode(bool),
    ShowSettings,
    /// Back to the programs page.
    ShowMain,
    /// Show / hide the log inside the Run dialog.
    ToggleRunLog,
    ShowModal(Modal),
    CloseModal,
}

impl Action {
    /// The ritz-get command this action runs, or `None` for UI-only / non-ritz-get actions.
    pub fn to_command(&self) -> Option<Command> {
        Some(match self.clone() {
            Action::Install { name, branch } => Command::Install { name, branch },
            Action::Repair { name } => Command::Install { name, branch: None },
            Action::Update { name } => Command::Update { name },
            Action::Remove { name } => Command::Remove { name },
            Action::Switch { name, branch } => Command::Switch { name, branch },
            Action::Reset { name } => Command::Reset { name },
            Action::Hold { name } => Command::Hold { name },
            Action::Unhold { name } => Command::Unhold { name },
            Action::Open { name } => Command::Open { name },
            Action::Shortcut { name, remove } => Command::Shortcut { name, remove },
            Action::SetDir(p) => Command::Dir { path: Some(p) },
            Action::SetShortcutsDir(p) => Command::Shortcuts { path: Some(p) },
            Action::CheckForUpdates => Command::List { offline: false, all: false },
            Action::Branches { name } => Command::Branches { name },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse() {
        assert_eq!(Version::parse("-"), None);
        let v = Version::parse("2026-10-14 3f9e21a").unwrap();
        assert_eq!(v.date.as_deref(), Some("2026-10-14"));
        assert_eq!(v.sha, "3f9e21a");
        assert_eq!(v.to_string(), "2026-10-14 3f9e21a");
        let v = Version::parse("4be8a30").unwrap();
        assert_eq!((v.date, v.sha.as_str()), (None, "4be8a30"));
    }

    #[test]
    fn command_args() {
        let a = |c: Command| c.args().join(" ");
        assert_eq!(a(Command::List { offline: true, all: false }), "list --offline --porcelain");
        assert_eq!(a(Command::List { offline: false, all: false }), "list --porcelain");
        assert_eq!(a(Command::Update { name: None }), "update --yes");
        assert_eq!(a(Command::Update { name: Some("x".into()) }), "update x --yes");
        assert_eq!(a(Command::Reset { name: "x".into() }), "reset x --yes");
        assert_eq!(a(Command::Install { name: "x".into(), branch: Some("dev".into()) }), "install x --branch dev");
        assert_eq!(a(Command::Shortcut { name: "x".into(), remove: true }), "shortcut x --remove");
        assert_eq!(a(Command::Shortcut { name: "x".into(), remove: false }), "shortcut x");
        // PORCELAIN: `--yes` always on `dir PATH`; `shortcuts PATH` takes none; the queries take no path.
        assert_eq!(a(Command::Dir { path: Some("/p".into()) }), "dir /p --yes");
        assert_eq!(a(Command::Dir { path: None }), "dir");
        assert_eq!(a(Command::Shortcuts { path: Some("/s".into()) }), "shortcuts /s");
        assert_eq!(a(Command::Shortcuts { path: None }), "shortcuts");
        assert!(Command::Remove { name: "x".into() }.uses_sudo());
        assert!(!Command::Update { name: None }.uses_sudo());
    }

    #[test]
    fn exit_kinds() {
        assert_eq!(ExitKind::from_code(Some(3)), ExitKind::Deps);
        assert_eq!(ExitKind::from_code(Some(5)), ExitKind::Busy);
        assert_eq!(ExitKind::from_code(Some(9)), ExitKind::Other(9));
        assert_eq!(ExitKind::from_code(None), ExitKind::Other(-1));
    }
}
