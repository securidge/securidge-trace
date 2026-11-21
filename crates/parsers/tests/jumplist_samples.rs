//! 점프 목록 파서를 공개 표본 파일로 확인한다.
//!
//! 표본과 기대값: Eric Zimmerman의 JumpList 라이브러리 테스트 (MIT, tests/data/ez-jumplist/LICENSE).
//! `9d1f905ce5044aee`, `5afe4de1b92fc382`는 libyal/dtformats의 시험 자료 (Apache-2.0, README-dtformats.md).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use st_parsers::jumplist;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ez-jumplist").join(name)).unwrap()
}

fn auto(name: &str) -> jumplist::JumpList {
    let id = jumplist::app_id_from_file_name(name);
    let id = id.trim_start_matches("win81-");
    jumplist::parse_automatic(&data(name), id, encoding_rs::EUC_KR).unwrap()
}

#[test]
fn windows10_explorer_entries() {
    let j = auto("f01b4d95cf55d32a.automaticDestinations-ms");
    assert_eq!(j.version, Some(3));
    assert_eq!(j.app_name.as_deref(), Some("Windows Explorer Windows 8.1"));
    let numbers: Vec<u32> = j.entries.iter().map(|e| e.entry_number).collect();
    assert_eq!(numbers, vec![7, 9, 8, 1, 2, 6, 5, 4, 3]);
    assert_eq!(j.entries.iter().filter(|e| e.pinned).count(), 4);
    let first = &j.entries[0];
    assert_eq!(first.path, r"C:\Temp");
    assert_eq!(first.hostname, "desktop-annfp9d");
    assert_eq!(first.interaction_count, Some(2));
    assert!(!first.pinned);
    assert_eq!(first.last_used.unwrap().to_rfc3339(), "2015-11-24T20:31:34.447525300+00:00");
    assert_eq!(first.drive_type.as_deref(), Some("fixed"));
    let pinned = j.entries.iter().find(|e| e.entry_number == 1).unwrap();
    assert!(pinned.pinned);
    assert_eq!(pinned.path, r"C:\Users\e\Desktop");
    assert!(j.notes.is_empty(), "{:?}", j.notes);
}

#[test]
fn counts_match_reference() {
    for (name, entries, version) in [
        ("5f7b5f1e01b83767.automaticDestinations-ms", 0usize, Some(0u32)),
        ("7e4dca80246863e3.automaticDestinations-ms", 1, Some(3)),
        ("1b4dd67f29cb1962.automaticDestinations-ms", 4, Some(1)),
        ("WIN81-f01b4d95cf55d32a.automaticDestinations-ms", 4, Some(1)),
        ("9d1f905ce5044aee.automaticDestinations-ms", 2, Some(3)),
    ] {
        let j = auto(name);
        assert_eq!(j.entries.len(), entries, "{name}");
        if entries > 0 {
            assert_eq!(j.version, version, "{name}");
        }
    }
    let edge = auto("9d1f905ce5044aee.automaticDestinations-ms");
    assert_eq!(edge.entries[0].path, "http://support.microsoft.com/kb/3124263");
    assert_eq!(edge.entries[0].last_used.unwrap().to_rfc3339(), "2016-01-17T13:08:08.247504500+00:00");
}

#[test]
fn custom_destinations() {
    let j = jumplist::parse_custom(&data("5afe4de1b92fc382.customDestinations-ms"), "5afe4de1b92fc382", encoding_rs::EUC_KR)
        .unwrap();
    assert_eq!(j.entries.len(), 9);
    assert!(j.entries.iter().all(|e| e.path == r"C:\Windows\System32\GettingStarted.exe"));
}

#[test]
fn damaged_files_never_panic() {
    let d = data("f01b4d95cf55d32a.automaticDestinations-ms");
    for cut in (0..d.len()).step_by(211) {
        let _ = jumplist::parse_automatic(&d[..cut], "x", encoding_rs::EUC_KR);
    }
    let mut flipped = d.clone();
    for i in (600..flipped.len()).step_by(17) {
        flipped[i] ^= 0xA5;
    }
    let _ = jumplist::parse_automatic(&flipped, "x", encoding_rs::EUC_KR);
}
