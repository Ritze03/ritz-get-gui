//! In-code sample data: the mockup's `SECTIONS` (17 entries over Software / Helpers / Websites /
//! Other) plus one `Unlisted` clone, as a parsed [`Listing`]. Used by the stub `App` until I7 wires
//! the real startup, by the pane-overflow test, and handy for any worker's unit tests.
//!
//! It covers: installed current / behind / modified / held, not installed (bare-sha `latest`),
//! entries with `S` setup steps, websites with shortcut on/off, program shortcut on/off/none, and
//! an unlisted clone. It deliberately does NOT cover offline / fetch-failed / unreachable (I2's
//! fixtures do).

use crate::model::*;

fn base(section: &str, name: &str, title: &str, desc: &str) -> Entry {
    Entry {
        section: section.into(),
        name: name.into(),
        title: (title != name).then(|| title.to_owned()),
        kind: Kind::Program,
        source: Some(format!("Ritze03/{name}")),
        state: State::Installed,
        status: Status::Current,
        held: false,
        branch: Some("main".into()),
        installed: None,
        latest: None,
        behind: None,
        dirty: None,
        pending_branch: None,
        ahead: None,
        desc: desc.into(),
        shortcut: ShortcutState::None,
        setup: Vec::new(),
        data: Vec::new(),
    }
}

/// Installed, up to date.
fn current(section: &str, name: &str, title: &str, desc: &str, ver: &str, sc: ShortcutState) -> Entry {
    let mut e = base(section, name, title, desc);
    e.installed = Version::parse(ver);
    e.latest = Version::parse(ver);
    e.behind = Some(0);
    e.dirty = Some(0);
    e.ahead = Some(0);
    e.shortcut = sc;
    e
}

/// Installed, behind upstream.
fn behind(section: &str, name: &str, title: &str, desc: &str, ver: &str, latest: &str, sc: ShortcutState) -> Entry {
    let mut e = current(section, name, title, desc, ver, sc);
    e.status = Status::Behind;
    e.latest = Version::parse(latest);
    e.behind = Some(3);
    e
}

/// Not installed (reachable): `latest` is a bare sha.
fn available(section: &str, name: &str, title: &str, desc: &str, sha: &str) -> Entry {
    let mut e = base(section, name, title, desc);
    e.state = State::Available;
    e.status = Status::None;
    e.latest = Version::parse(sha);
    e.branch = None;
    e.shortcut = ShortcutState::None;
    e
}

fn website(section: &str, name: &str, title: &str, desc: &str, url: &str, sc: ShortcutState) -> Entry {
    let mut e = base(section, name, title, desc);
    e.kind = Kind::Website;
    e.source = Some(url.into());
    e.state = State::Website;
    e.status = Status::None;
    e.branch = None;
    e.shortcut = sc;
    e
}

fn setup(mut e: Entry, steps: &[&str]) -> Entry {
    e.setup = steps.iter().map(|t| SetupStep { text: (*t).into() }).collect();
    e
}

fn with_data(mut e: Entry, data: &[(&str, u64)]) -> Entry {
    e.data = data.iter().map(|(p, kb)| DataPath { path: (*p).into(), kb: Some(*kb) }).collect();
    e
}

pub fn listing() -> Listing {
    use ShortcutState::{None as NoSc, Off, On};
    let mut yt = behind(
        "Software", "yt-dlp-gui", "yt-dlp GUI", "Lean Go + Gio desktop GUI for yt-dlp",
        "2026-10-14 3f9e21a", "2026-10-21 c84d0b7", On,
    );
    yt.held = true;
    let mut geo = behind(
        "Software", "geofinder", "Geofinder", "Geobingo-style multiplayer party game with real Google and Apple Maps",
        "2026-10-01 2d81f06", "2026-10-08 9c44d1a", On,
    );
    geo.status = Status::Modified;
    geo.dirty = Some(3);
    let mut ritz_get_gui = behind(
        "Helpers", "ritz-get-gui", "Ritz-Get-GUI", "Graphical frontend for ritz-get",
        "2026-10-19 5e7a2c1", "2026-10-22 d40f9b8", On,
    );
    ritz_get_gui.source = Some("Ritze03/ritz-get-gui".into());
    let mut forza = available(
        "Software", "ForzaTelemetryV3Launcher", "Forza Telemetry V3 Launcher",
        "Launcher for Forza Telemetry V3: keeps it up to date, builds and runs it", "433d3ad",
    );
    forza.branch = Some("master".into());
    let mut unlisted = current("Unlisted", "old-experiment", "old-experiment", "", "2026-09-02 77aa001", NoSc);
    unlisted.state = State::Unlisted;
    unlisted.source = None;

    let sections = vec![
        Section {
            name: "Software".into(),
            entries: vec![
                yt,
                with_data(
                    available("Software", "ritz", "Ritz", "Linux Steam launch wrapper", "9e24960"),
                    &[("~/.config/ritz", 12288), ("~/.local/share/ritz", 2048)],
                ),
                forza,
                setup(
                    available(
                        "Software", "ForzaTelemetryV3", "Forza Telemetry V3",
                        "Live telemetry dashboard for Forza Horizon (the app itself, no launcher)", "1821a51",
                    ),
                    &["Hotkeys, gearbox and backfire need the input group: sudo usermod -aG input $USER, then log in again"],
                ),
                setup(
                    available(
                        "Software", "simple-screensharing", "Simple Screensharing",
                        "Friends screensharing: native desktop client (E2E-encrypted, low latency)", "f6431b8",
                    ),
                    &[
                        "Needs PipeWire (incl. pw-loopback) and GStreamer plugins pipewire/good/bad/libav at runtime",
                        "Needs a relay: self-host from this repo, or use a friend's; the app takes a join link",
                    ],
                ),
                geo,
                with_data(
                    current("Software", "SteamAlt", "Steam Userdata Copier", "Copy Steam game saves and settings between accounts on one PC", "2026-10-17 8a3f1c9", On),
                    &[("~/.config/SteamAlt", 340)],
                ),
                setup(
                    current(
                        "Software", "gamescope-ritz", "Gamescope Ritz",
                        "SteamOS session compositing window manager, with added features", "2026-10-09 0b7c5e2", NoSc,
                    ),
                    &["Ritz users: copy its Ritz module by hand, see https://github.com/Ritze03/gamescope-ritz#quick-start"],
                ),
            ],
        },
        Section {
            name: "Helpers".into(),
            entries: vec![
                current("Helpers", "ritz-get", "ritz-get", "Install, update and remove Ritze03's programs, built from source", "2026-10-22 a596443", NoSc),
                ritz_get_gui,
                current("Helpers", "ccorral", "ccorral", "Keeps Claude Code sessions on a few CPU cores so the desktop stays fast", "2026-10-18 7d1e0f2", Off),
                setup(
                    behind(
                        "Helpers", "ts6tray", "TeamSpeak 6 Tray", "Tray icon, mute control and notifications for TeamSpeak 6",
                        "2026-10-20 a596443", "2026-10-22 b12c3d4", NoSc,
                    ),
                    &[
                        "In TeamSpeak: Settings → Remote Apps → enable, port 5899",
                        "First start asks TeamSpeak for approval. Details: https://github.com/Ritze03/ts6tray#quick-start",
                    ],
                ),
            ],
        },
        Section {
            name: "Websites".into(),
            entries: vec![
                website("Websites", "cs2-alias-cycling", "CS2 Alias Cycling", "CS2 alias cycling config generator", "https://ritze03.github.io/stupid-pages/cs2-alias-cycling-generator.html", Off),
                website("Websites", "cs2-chat-keybind", "CS2 Chat Keybind", "CS2 chat keybind config generator", "https://ritze03.github.io/stupid-pages/cs2-chat-keybind-generator.html", Off),
                website("Websites", "cs2-multimap", "CS2 Multimap", "CS2 multimap config generator", "https://ritze03.github.io/stupid-pages/cs2-multimap-generator.html", Off),
                website("Websites", "simple-screensharing-web", "Simple Screensharing (Web)", "Watch or share a screen in the browser, no install", "https://ritze03.github.io/simple-screensharing/", Off),
                website("Websites", "walters-knaggertreff", "Walters Knaggertreff", "Party board game full of German memes, 2-6 players in the browser", "https://ritze03.github.io/walters-knaggertreff/", On),
            ],
        },
        Section {
            name: "Other".into(),
            entries: vec![website("Other", "ts6-themes", "TeamSpeak 6 Themes", "Custom themes for the TeamSpeak 6 client", "https://ritze03.github.io/Teamspeak6Themes/", Off)],
        },
        Section { name: Listing::UNLISTED.into(), entries: vec![unlisted] },
    ];
    Listing { version: 1, online: Some(true), hidden: 0, sections }
}
