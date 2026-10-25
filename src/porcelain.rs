//! `ritz-get list --porcelain` parser (**I2**).
//!
//! > I2: Porcelain parser per PORCELAIN.md: 16- or 17-field `E` with `shortcut` defaulting to `-`,
//! > `S`, `#online`, `#hidden`, `Unlisted`, `offline` state, `unknown` status, bare-sha `latest`.
//! > Fixtures in its exact format: `full`, `offline`, `hidden`, `unlisted`, `legacy-16`,
//! > `bad-version`. Unit tests.
//!
//! All porcelain parsing lives in this one module (D50). The result types, field mapping and the
//! small `parse` helpers (`Kind::parse`, `State::parse`, `Status::parse`, `ShortcutState::parse`,
//! `Version::parse`) are in `crate::model`; do not redefine them.
//!
//! Notes for the implementer:
//! - "16 fields" in PORCELAIN.md counts the leading `E` tag: tag + 15 data columns; a 17-field
//!   line has the extra trailing `shortcut` column.
//! - Fields are TAB-separated; `desc` may be empty (trailing empty field).
//! - `S` and `D` lines attach to the preceding `E` (`D`: data folders, only with `list --data`). `#porcelain N` must be the first line; N != 1 ->
//!   [`PorcelainError::UnsupportedVersion`].
//! - `Entry::title` is `None` when the column is empty or equals `name`.

use std::fmt;

use crate::model::{DataPath, Entry, Kind, Listing, Section, SetupStep, ShortcutState, State, Status, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PorcelainError {
    /// The text is empty / has no `#porcelain` first line.
    MissingHeader,
    /// `#porcelain N` with N != 1: refuse and offer "update ritz-get-gui" (D32).
    UnsupportedVersion(u32),
    /// A malformed line (1-based line number).
    BadLine { line: usize, message: String },
}

impl fmt::Display for PorcelainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PorcelainError::MissingHeader => write!(f, "missing #porcelain header"),
            PorcelainError::UnsupportedVersion(v) => write!(f, "unsupported porcelain version {v}"),
            PorcelainError::BadLine { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for PorcelainError {}

/// Parse the full stdout of `ritz-get list --porcelain`.
///
/// - `#porcelain N` must be the first non-blank line; only `1` is accepted.
/// - `#online`, `#hidden` are read wherever they appear; other `#` lines and blank lines are ignored.
/// - `E` has 16 fields (no `shortcut` column, D4), 17, or more (extra columns ignored); fewer than 16
///   is a [`PorcelainError::BadLine`].
/// - `S` attaches to the `E` before it (its `name` must match); a stray `S` is a `BadLine`.
/// - Sections keep first-seen order (D8); `Unlisted` is moved last.
pub fn parse(text: &str) -> Result<Listing, PorcelainError> {
    let bad = |line: usize, message: String| PorcelainError::BadLine { line, message };
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim_end_matches('\r'))).filter(|(_, l)| !l.trim().is_empty());

    // Header.
    let Some((hn, header)) = lines.next() else { return Err(PorcelainError::MissingHeader) };
    let version = match directive(header) {
        Some(("porcelain", arg)) => arg.parse::<u32>().map_err(|_| bad(hn, format!("bad #porcelain version {arg:?}")))?,
        _ => return Err(PorcelainError::MissingHeader),
    };
    if version != 1 {
        return Err(PorcelainError::UnsupportedVersion(version));
    }

    let mut listing = Listing { version, ..Listing::default() };
    // (section index, entry index) of the last `E`, which following `S` lines attach to.
    let mut last: Option<(usize, usize)> = None;

    for (n, line) in lines {
        if let Some((key, arg)) = directive(line) {
            match key {
                "online" => {
                    listing.online = Some(match arg {
                        "1" => true,
                        "0" => false,
                        _ => return Err(bad(n, format!("bad #online value {arg:?}"))),
                    })
                }
                "hidden" => listing.hidden = arg.parse().map_err(|_| bad(n, format!("bad #hidden value {arg:?}")))?,
                _ => {} // unknown directive / comment: ignored
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "E" => {
                let entry = parse_entry(&f).map_err(|m| bad(n, m))?;
                let si = match listing.sections.iter().position(|s| s.name == entry.section) {
                    Some(i) => i,
                    None => {
                        listing.sections.push(Section { name: entry.section.clone(), entries: Vec::new() });
                        listing.sections.len() - 1
                    }
                };
                listing.sections[si].entries.push(entry);
                last = Some((si, listing.sections[si].entries.len() - 1));
            }
            "S" => {
                if f.len() != 3 {
                    return Err(bad(n, format!("S line has {} fields, expected 3", f.len())));
                }
                let Some((si, ei)) = last else { return Err(bad(n, "S line without a preceding E line".into())) };
                let entry = &mut listing.sections[si].entries[ei];
                if entry.name != f[1] {
                    return Err(bad(n, format!("S line for {:?} follows E line for {:?}", f[1], entry.name)));
                }
                entry.setup.push(SetupStep { text: f[2].to_owned() });
            }
            "D" => {
                // D<TAB>NAME<TAB>PATH<TAB>KB: an existing data folder (`list --data`). Does not touch `last`.
                if f.len() != 4 {
                    return Err(bad(n, format!("D line has {} fields, expected 4", f.len())));
                }
                let Some((si, ei)) = last else { return Err(bad(n, "D line without a preceding E line".into())) };
                let entry = &mut listing.sections[si].entries[ei];
                if entry.name != f[1] {
                    return Err(bad(n, format!("D line for {:?} follows E line for {:?}", f[1], entry.name)));
                }
                entry.data.push(DataPath { path: f[2].to_owned(), kb: f[3].parse().ok() });
            }
            _ => {} // unknown record type: ignored, so newer ritz-get records don't break older GUIs
        }
    }

    // Unlisted always last (stable: other sections keep their order).
    listing.sections.sort_by_key(|s| s.name == Listing::UNLISTED);
    Ok(listing)
}

/// `#key arg` -> `("key", "arg")` (`arg` may be empty). `None` when the line is not a `#` line.
fn directive(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('#')?;
    let (key, arg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((key, arg.trim()))
}

fn parse_entry(f: &[&str]) -> Result<Entry, String> {
    // Columns appended after 17 by a later ritz-get are ignored (PORCELAIN.md: appended columns keep version 1).
    if f.len() < 16 {
        return Err(format!("E line has {} fields, expected at least 16", f.len()));
    }
    let name = f[2];
    if f[1].is_empty() || name.is_empty() {
        return Err("E line with empty section or name".into());
    }
    let kind = Kind::parse(f[4]).ok_or_else(|| format!("unknown kind {:?}", f[4]))?;
    let held = match f[8] {
        "1" => true,
        "0" => false,
        v => return Err(format!("bad held value {v:?}")),
    };
    let count = |what: &str, v: &str| -> Result<Option<u32>, String> {
        if v == "-" {
            Ok(None)
        } else {
            v.parse().map(Some).map_err(|_| format!("bad {what} count {v:?}"))
        }
    };
    let dash = |v: &str| (!v.is_empty() && v != "-").then(|| v.to_owned());
    Ok(Entry {
        section: f[1].to_owned(),
        name: name.to_owned(),
        title: (!f[3].is_empty() && f[3] != name).then(|| f[3].to_owned()),
        kind,
        source: dash(f[5]),
        state: State::parse(f[6]),
        status: Status::parse(f[7]),
        held,
        branch: dash(f[9]),
        installed: Version::parse(f[10]),
        latest: Version::parse(f[11]),
        behind: count("behind", f[12])?,
        dirty: count("dirty", f[13])?,
        ahead: count("ahead", f[14])?,
        desc: f[15].to_owned(),
        shortcut: f.get(16).map_or(ShortcutState::None, |s| ShortcutState::parse(s)),
        setup: Vec::new(),
        data: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Listing, ShortcutState, State, Status};

    const FULL: &str = include_str!("../tests/fixtures/full.porcelain");
    const OFFLINE: &str = include_str!("../tests/fixtures/offline.porcelain");
    const HIDDEN: &str = include_str!("../tests/fixtures/hidden.porcelain");
    const UNLISTED: &str = include_str!("../tests/fixtures/unlisted.porcelain");
    const LEGACY: &str = include_str!("../tests/fixtures/legacy-16.porcelain");
    const BAD_VERSION: &str = include_str!("../tests/fixtures/bad-version.porcelain");

    fn find<'a>(l: &'a Listing, name: &str) -> &'a Entry {
        l.entries().find(|e| e.name == name).unwrap_or_else(|| panic!("no entry {name}"))
    }

    fn section_names(l: &Listing) -> Vec<&str> {
        l.sections.iter().map(|s| s.name.as_str()).collect()
    }

    /// One E line with 17 fields; `over` replaces fields by index (0 = the "E" tag).
    fn e_line(over: &[(usize, &str)]) -> String {
        let mut f = vec![
            "E", "Software", "foo", "Foo", "program", "o/foo", "installed", "current", "0", "main",
            "2026-10-14 3f9e21a", "2026-10-14 3f9e21a", "0", "0", "0", "desc", "on",
        ];
        for (i, v) in over {
            f[*i] = v;
        }
        f.join("\t")
    }

    fn doc(body: &str) -> String {
        format!("#porcelain 1\n#online 1\n{body}\n")
    }

    #[test]
    fn every_fixture_parses() {
        for (name, text, n) in [("full", FULL, 19), ("offline", OFFLINE, 19), ("hidden", HIDDEN, 5), ("unlisted", UNLISTED, 6), ("legacy-16", LEGACY, 19)] {
            let l = parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(l.version, 1, "{name}");
            assert_eq!(l.entries().count(), n, "{name}");
        }
        assert_eq!(parse(BAD_VERSION), Err(PorcelainError::UnsupportedVersion(2)));
    }

    #[test]
    fn full_matches_sample_listing() {
        assert_eq!(parse(FULL).unwrap(), crate::sample::listing());
    }

    #[test]
    fn full_sections_and_cases() {
        let l = parse(FULL).unwrap();
        assert_eq!(section_names(&l), ["Software", "Helpers", "Websites", "Other", "Unlisted"]);
        assert_eq!(l.online, Some(true));
        assert_eq!(l.hidden, 0);
        let yt = find(&l, "yt-dlp-gui");
        assert!(yt.held && yt.status == Status::Behind && !yt.updatable());
        let geo = find(&l, "geofinder");
        assert_eq!((geo.status.clone(), geo.dirty, geo.behind), (Status::Modified, Some(3), Some(3)));
        assert!(geo.has_update() && !geo.updatable());
        assert_eq!(find(&l, "ritz-get-gui").status, Status::Behind);
    }

    #[test]
    fn s_lines_attach_to_preceding_entry() {
        let l = parse(FULL).unwrap();
        let ss = find(&l, "simple-screensharing");
        assert_eq!(ss.setup.len(), 2);
        assert!(ss.setup[0].text.starts_with("Needs PipeWire") && ss.setup[1].text.contains("join link"));
        assert_eq!(find(&l, "gamescope-ritz").setup.len(), 1);
        let tray = find(&l, "ts6tray");
        assert_eq!(tray.setup.len(), 2);
        assert!(tray.setup[0].text.starts_with("In TeamSpeak"));
        assert!(tray.setup[1].text.contains("https://github.com/Ritze03/ts6tray#quick-start"));
        assert_eq!(l.entries().map(|e| e.setup.len()).sum::<usize>(), 6);
        assert!(find(&l, "ForzaTelemetryV3").setup[0].text.starts_with("Hotkeys, gearbox"));
        // the entry after an entry with S lines gets none
        assert!(find(&l, "ForzaTelemetryV3Launcher").setup.is_empty());
    }

    #[test]
    fn unknown_record_types_are_ignored() {
        let t = doc(&format!("{}\nD\tfoo\t~/.config/foo\t128\nS\tfoo\tstep one\nZ\tfoo\nS\tfoo\tstep two", e_line(&[])));
        let l = parse(&t).unwrap();
        assert_eq!(l.entries().count(), 1);
        let foo = find(&l, "foo");
        assert_eq!(foo.setup.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["step one", "step two"]);
        assert_eq!(foo.data, [DataPath { path: "~/.config/foo".into(), kb: Some(128) }]);
    }

    #[test]
    fn d_records_parse_after_e_and_s() {
        let t = doc(&format!(
            "{}\nS\tfoo\tstep\nD\tfoo\t~/.config/foo\t12288\nD\tfoo\t~/.local/share/foo\t-\n{}\nD\tbar\t~/.bar\t5",
            e_line(&[]),
            e_line(&[(2, "bar")])
        ));
        let l = parse(&t).unwrap();
        let foo = find(&l, "foo");
        assert_eq!(foo.setup.len(), 1);
        assert_eq!(
            foo.data,
            [
                DataPath { path: "~/.config/foo".into(), kb: Some(12288) },
                DataPath { path: "~/.local/share/foo".into(), kb: None },
            ]
        );
        assert_eq!(foo.data_label().as_deref(), Some("2 folders, 12 MB"));
        assert_eq!(find(&l, "bar").data, [DataPath { path: "~/.bar".into(), kb: Some(5) }]);
    }

    #[test]
    fn bad_d_lines() {
        let t = "#porcelain 1\n#online 1\nD\tfoo\t~/x\t1\n";
        assert!(matches!(parse(t), Err(PorcelainError::BadLine { line: 3, .. })));
        let t = doc(&format!("{}\nD\tother\t~/x\t1", e_line(&[])));
        assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 4, .. })));
        let t = doc(&format!("{}\nD\tfoo\t~/x", e_line(&[])));
        assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 4, .. })));
        let t = doc(&format!("{}\nD\tfoo\t~/x\t1\textra", e_line(&[])));
        assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 4, .. })));
    }

    #[test]
    fn stray_or_mismatched_s_is_bad_line() {
        let t = "#porcelain 1\n#online 1\nS\tfoo\tstep\n";
        assert!(matches!(parse(t), Err(PorcelainError::BadLine { line: 3, .. })));
        let t = doc(&format!("{}\nS\tother\tstep", e_line(&[])));
        assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 4, .. })));
        let t = doc(&format!("{}\nS\tfoo", e_line(&[])));
        assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 4, .. })));
    }

    #[test]
    fn shortcut_column_17_and_16() {
        let l = parse(FULL).unwrap();
        assert_eq!(find(&l, "SteamAlt").shortcut, ShortcutState::On);
        assert_eq!(find(&l, "ccorral").shortcut, ShortcutState::Off);
        assert_eq!(find(&l, "gamescope-ritz").shortcut, ShortcutState::None);
        assert_eq!(find(&l, "ritz").shortcut, ShortcutState::None); // not installed
        assert_eq!(find(&l, "walters-knaggertreff").shortcut, ShortcutState::On);
        assert_eq!(find(&l, "cs2-multimap").shortcut, ShortcutState::Off);
        for e in l.entries().filter(|e| e.is_website()) {
            assert_ne!(e.shortcut, ShortcutState::None, "{}", e.name);
        }
        assert_eq!(find(&l, "old-experiment").shortcut, ShortcutState::None);

        let legacy = parse(LEGACY).unwrap();
        assert!(legacy.entries().all(|e| e.shortcut == ShortcutState::None));
        // everything except the shortcut column is identical to the 17-field listing
        let mut expect = l.clone();
        for s in &mut expect.sections {
            for e in &mut s.entries {
                e.shortcut = ShortcutState::None;
            }
        }
        assert_eq!(legacy, expect);
        // inline: 16 fields, then 17 with each value
        let line16 = e_line(&[]).rsplit_once('\t').unwrap().0.to_owned();
        assert_eq!(parse(&doc(&line16)).unwrap().entries().next().unwrap().shortcut, ShortcutState::None);
        for (v, want) in [("on", ShortcutState::On), ("off", ShortcutState::Off), ("-", ShortcutState::None)] {
            let l = parse(&doc(&e_line(&[(16, v)]))).unwrap();
            assert_eq!(l.entries().next().unwrap().shortcut, want, "{v}");
        }
    }

    #[test]
    fn bad_field_counts_report_line_number() {
        let line16 = e_line(&[]).rsplit_once('\t').unwrap().0.to_owned();
        let line15 = line16.rsplit_once('\t').unwrap().0.to_owned();
        for bad in [line15, "E\tSoftware\tfoo".to_owned()] {
            // good entry on line 3, bad one on line 4
            let t = doc(&format!("{}\n{bad}", e_line(&[])));
            match parse(&t) {
                Err(PorcelainError::BadLine { line: 4, message }) => assert!(message.contains("fields"), "{message}"),
                other => panic!("{other:?}"),
            }
        }
        // 16 is fine
        assert!(parse(&doc(&line16)).is_ok());
        // 18+ fields: appended columns are ignored, shortcut still comes from column 17
        for extra in ["\textra", "\textra\tmore"] {
            let l = parse(&doc(&format!("{}{extra}", e_line(&[(16, "off")])))).unwrap();
            let e = l.entries().next().unwrap();
            assert_eq!((e.name.as_str(), e.shortcut), ("foo", ShortcutState::Off));
        }
    }

    #[test]
    fn bad_values_are_bad_lines() {
        for over in [&[(4, "plugin")][..], &[(8, "yes")], &[(12, "x")], &[(13, "-1")], &[(2, "")]] {
            let t = doc(&e_line(over));
            assert!(matches!(parse(&t), Err(PorcelainError::BadLine { line: 3, .. })), "{over:?}");
        }
        assert!(matches!(parse("#porcelain 1\n#online maybe\n"), Err(PorcelainError::BadLine { line: 2, .. })));
        assert!(matches!(parse("#porcelain 1\n#hidden many\n"), Err(PorcelainError::BadLine { line: 2, .. })));
        assert_eq!(parse("#porcelain 1\nX\tfoo\n").unwrap().entries().count(), 0); // unknown record type: ignored
        assert!(matches!(parse("#porcelain x\n"), Err(PorcelainError::BadLine { line: 1, .. })));
    }

    #[test]
    fn header_rules() {
        assert_eq!(parse(""), Err(PorcelainError::MissingHeader));
        assert_eq!(parse("\n\n"), Err(PorcelainError::MissingHeader));
        assert_eq!(parse(&format!("#online 1\n{}\n", e_line(&[]))), Err(PorcelainError::MissingHeader));
        assert_eq!(parse(&e_line(&[])), Err(PorcelainError::MissingHeader));
        assert_eq!(parse("#porcelain 0\n"), Err(PorcelainError::UnsupportedVersion(0)));
        assert_eq!(parse("#porcelain 2\ngarbage\n"), Err(PorcelainError::UnsupportedVersion(2)));
        let empty = parse("#porcelain 1\n").unwrap();
        assert_eq!((empty.version, empty.online, empty.hidden, empty.sections.len()), (1, None, 0, 0));
        assert!(!empty.is_offline());
    }

    #[test]
    fn states_and_statuses() {
        let t = doc(
            &[
                e_line(&[(2, "a"), (6, "installed"), (7, "current")]),
                e_line(&[(2, "b"), (6, "installed"), (7, "behind")]),
                e_line(&[(2, "c"), (6, "installed"), (7, "modified")]),
                e_line(&[(2, "d"), (6, "installed"), (7, "fetch-failed")]),
                e_line(&[(2, "e"), (6, "installed"), (7, "unknown")]),
                e_line(&[(2, "f"), (6, "available"), (7, "-")]),
                e_line(&[(2, "g"), (6, "unreachable"), (7, "-")]),
                e_line(&[(2, "h"), (6, "offline"), (7, "-")]),
                e_line(&[(2, "i"), (4, "website"), (6, "website"), (7, "-")]),
                e_line(&[(2, "j"), (6, "unlisted"), (7, "current")]),
                e_line(&[(2, "k"), (6, "quantum"), (7, "weird")]),
            ]
            .join("\n"),
        );
        let l = parse(&t).unwrap();
        let got: Vec<(State, Status)> = l.entries().map(|e| (e.state.clone(), e.status.clone())).collect();
        use State as S;
        use Status as T;
        assert_eq!(
            got,
            vec![
                (S::Installed, T::Current),
                (S::Installed, T::Behind),
                (S::Installed, T::Modified),
                (S::Installed, T::FetchFailed),
                (S::Installed, T::Unknown),
                (S::Available, T::None),
                (S::Unreachable, T::None),
                (S::Offline, T::None),
                (S::Website, T::None),
                (S::Unlisted, T::Current),
                (S::Other("quantum".into()), T::Other("weird".into())),
            ]
        );
    }

    #[test]
    fn offline_fixture() {
        let l = parse(OFFLINE).unwrap();
        assert_eq!(l.online, Some(false));
        assert!(l.is_offline());
        for e in l.entries() {
            match &e.state {
                State::Offline => {
                    assert_eq!((&e.status, &e.latest, &e.installed), (&Status::None, &None, &None), "{}", e.name)
                }
                State::Installed | State::Unlisted => {
                    assert!(matches!(e.status, Status::Unknown | Status::Modified), "{}: {:?}", e.name, e.status);
                    assert_eq!((&e.latest, e.behind), (&None, None), "{}", e.name);
                    assert!(e.installed.is_some());
                }
                State::Website => {}
                s => panic!("{}: unexpected state {s:?}", e.name),
            }
        }
        assert_eq!(find(&l, "ritz").state, State::Offline);
        assert_eq!(find(&l, "SteamAlt").status, Status::Unknown);
        assert_eq!(find(&l, "geofinder").status, Status::Modified);
        // same entries as the online listing
        let online = parse(FULL).unwrap();
        let ids = |l: &Listing| l.entries().map(|e| e.id()).collect::<Vec<_>>();
        assert_eq!(ids(&l), ids(&online));
    }

    #[test]
    fn hidden_fixture() {
        let l = parse(HIDDEN).unwrap();
        assert_eq!(l.hidden, 2);
        // hidden programs are not rendered or counted (D24): only the 5 listed entries exist
        assert_eq!(l.entries().count(), 5);
        assert_eq!(find(&l, "SteamAlt").status, Status::FetchFailed);
        assert_eq!(find(&l, "SteamAlt").latest, None);
        assert_eq!(parse(FULL).unwrap().hidden, 0);
        // #hidden may sit anywhere
        let l = parse("#porcelain 1\n#hidden 7\n#online 0\n").unwrap();
        assert_eq!((l.hidden, l.online), (7, Some(false)));
    }

    #[test]
    fn unlisted_last_with_dash_source_and_no_s_lines() {
        let l = parse(UNLISTED).unwrap();
        assert_eq!(section_names(&l), ["Software", "Helpers", "Unlisted"]);
        let u = &l.sections.last().unwrap().entries;
        assert_eq!(u.len(), 3);
        for e in u {
            assert_eq!((e.state.clone(), e.source.clone(), e.setup.len()), (State::Unlisted, None, 0), "{}", e.name);
            assert!(e.is_installed() && e.is_unlisted());
            assert_eq!(e.repo_url(), None);
        }
        let scratch = find(&l, "scratch-tool");
        assert_eq!((scratch.status.clone(), scratch.dirty, scratch.ahead), (Status::Modified, Some(2), Some(1)));
        assert_eq!(scratch.branch.as_deref(), Some("wip"));
        let fork = find(&l, "fork-of-something");
        assert!(fork.held && fork.has_update());
        // out-of-order input: Unlisted is moved last, other sections keep first-seen order
        let t = doc(
            &[
                e_line(&[(1, "Unlisted"), (2, "u"), (5, "-"), (6, "unlisted")]),
                e_line(&[(1, "Zed"), (2, "z")]),
                e_line(&[(1, "Alpha"), (2, "a")]),
                e_line(&[(1, "Zed"), (2, "z2")]),
            ]
            .join("\n"),
        );
        let l = parse(&t).unwrap();
        assert_eq!(section_names(&l), ["Zed", "Alpha", "Unlisted"]);
        assert_eq!(l.sections[0].entries.len(), 2);
    }

    #[test]
    fn bare_sha_latest_and_versions() {
        let l = parse(FULL).unwrap();
        let ritz = find(&l, "ritz");
        assert_eq!(ritz.state, State::Available);
        let latest = ritz.latest.as_ref().unwrap();
        assert_eq!((latest.date.as_deref(), latest.sha.as_str()), (None, "9e24960"));
        assert_eq!(ritz.installed, None);
        assert_eq!((ritz.behind, ritz.dirty, ritz.ahead), (None, None, None));
        let yt = find(&l, "yt-dlp-gui");
        assert_eq!(yt.installed.as_ref().unwrap().to_string(), "2026-10-14 3f9e21a");
        assert_eq!(yt.latest.as_ref().unwrap().to_string(), "2026-10-21 c84d0b7");
        let site = find(&l, "cs2-multimap");
        assert_eq!((site.installed.clone(), site.latest.clone(), site.branch.clone()), (None, None, None));
        assert_eq!(site.website_url(), Some("https://ritze03.github.io/stupid-pages/cs2-multimap-generator.html"));
    }

    #[test]
    fn title_fallback() {
        let l = parse(FULL).unwrap();
        assert_eq!(find(&l, "ccorral").title, None); // title == name
        assert_eq!(find(&l, "ccorral").display(), "ccorral");
        assert_eq!(find(&l, "ritz").title.as_deref(), Some("Ritz"));
        assert_eq!(find(&l, "ritz").display(), "Ritz");
        let l = parse(&doc(&e_line(&[(3, "")]))).unwrap();
        let e = l.entries().next().unwrap();
        assert_eq!((e.title.clone(), e.display()), (None, "foo"));
    }

    #[test]
    fn empty_desc_and_dash_fields() {
        let l = parse(&doc(&e_line(&[(15, ""), (9, "-"), (5, "-")]))).unwrap();
        let e = l.entries().next().unwrap();
        assert_eq!((e.desc.as_str(), e.branch.clone(), e.source.clone()), ("", None, None));
        assert_eq!(e.shortcut, ShortcutState::On); // empty desc does not shift the shortcut column
        // 16-field line whose desc is empty keeps its trailing empty field
        let t = doc(&format!("{}\t", e_line(&[(15, "")]).rsplit_once('\t').unwrap().0.rsplit_once('\t').unwrap().0));
        assert!(parse(&t).is_ok());
    }

    #[test]
    fn tolerates_crlf_blank_lines_and_unknown_directives() {
        let t = format!(
            "\r\n#porcelain 1\r\n#online 1\r\n\r\n# a comment\r\n#future-directive 5\r\n{}\r\nS\tfoo\tstep one\r\n\r\n#hidden 3\r\n",
            e_line(&[])
        );
        let l = parse(&t).unwrap();
        assert_eq!(l.hidden, 3);
        let e = l.entries().next().unwrap();
        assert_eq!(e.setup[0].text, "step one");
        assert_eq!(e.shortcut, ShortcutState::On);
        // no trailing newline
        assert!(parse(&format!("#porcelain 1\n{}", e_line(&[]))).is_ok());
    }

    #[test]
    fn error_display() {
        assert_eq!(PorcelainError::UnsupportedVersion(2).to_string(), "unsupported porcelain version 2");
        let e = parse("#porcelain 1\nE\tx\n").unwrap_err();
        assert!(e.to_string().starts_with("line 2: "));
    }
}
