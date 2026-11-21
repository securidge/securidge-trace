//! Prefetch 파서를 공개 표본 파일로 확인한다.
//!
//! 표본과 기대값: Eric Zimmerman의 Prefetch 라이브러리 테스트 (MIT, tests/data/ez-prefetch/LICENSE).
//! 기대 시각은 그 테스트의 값(-07:00 표기)을 UTC로 바꾼 것이다.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use st_parsers::prefetch;

fn read(name: &str) -> prefetch::PrefetchInfo {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ez-prefetch").join(name);
    let data = std::fs::read(&p).unwrap();
    prefetch::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn windows10_chrome() {
    let pf = read("CHROME.EXE-B3BA7868.pf");
    assert_eq!(pf.version, 30);
    assert_eq!(pf.executable, "CHROME.EXE");
    assert_eq!(pf.hash, "B3BA7868");
    assert_eq!(pf.run_count, 20);
    assert_eq!(pf.last_runs.len(), 8);
    assert_eq!(pf.last_runs[0].to_rfc3339(), "2016-01-13T18:06:55.334457700+00:00");
    assert_eq!(pf.volumes.len(), 1);
    assert_eq!(pf.volumes[0].device_path, r"\VOLUME{01d1217a9c4c6779-8c9f49ec}");
    assert_eq!(pf.volumes[0].serial, "8C9F-49EC");
    assert_eq!(pf.volumes[0].created.unwrap().to_rfc3339(), "2015-11-17T20:57:46.243468100+00:00");
    assert_eq!(pf.files.len(), 282);
    assert_eq!(pf.files[3], r"\VOLUME{01d1217a9c4c6779-8c9f49ec}\WINDOWS\SYSTEM32\KERNEL32.DLL");
    let exe = pf.executable_path.as_deref().unwrap();
    assert!(exe.ends_with(r"\CHROME.EXE"), "{exe}");
    assert_eq!(pf.executable_volume().unwrap().serial, "8C9F-49EC");
}

#[test]
fn windows10_two_volumes_and_long_name() {
    let pf = read("DCODEDCODEDCODEDCODEDCODEDCOD-E65B9FE8.pf");
    assert_eq!(pf.executable, "DCODEDCODEDCODEDCODEDCODEDCOD");
    assert_eq!(pf.run_count, 2);
    assert_eq!(pf.last_runs[0].to_rfc3339(), "2016-01-13T22:47:25.748075900+00:00");
    assert_eq!(pf.volumes.len(), 2);
    assert_eq!(pf.volumes[0].serial, "66F4-51BC");
    assert_eq!(pf.volumes[1].serial, "8C9F-49EC");
    assert_eq!(pf.files.len(), 57);
}

#[test]
fn windows10_devenv() {
    let pf = read("DEVENV.EXE-854D7862.pf");
    assert_eq!(pf.run_count, 54);
    assert_eq!(pf.last_runs[0].to_rfc3339(), "2016-01-13T16:50:34.657841600+00:00");
    let cmd = read("CMD.EXE-D269B812.pf");
    assert_eq!(cmd.executable, "CMD.EXE");
    assert!(cmd.run_count > 0);
}

#[test]
fn windows7_and_8() {
    let w7 = read("WIN7-DCODE-9054DA3F.pf");
    assert_eq!(w7.version, 23);
    assert_eq!(w7.run_count, 5);
    assert_eq!(w7.last_runs.len(), 1);
    assert_eq!(w7.last_runs[0].to_rfc3339(), "2016-01-22T16:23:16.341625+00:00");
    assert_eq!(w7.volumes[0].device_path, r"\DEVICE\HARDDISKVOLUME2");
    assert_eq!(w7.volumes[0].serial, "8800-8C2F");

    let w8 = read("WIN8-CALC.EXE-77FDF17F.pf");
    assert_eq!(w8.version, 26);
    assert_eq!(w8.executable, "CALC.EXE");
    assert_eq!(w8.run_count, 2);
    assert_eq!(w8.last_runs[0].to_rfc3339(), "2016-01-16T21:10:26.058341700+00:00");
    assert_eq!(w8.volumes[0].serial, "C6EE-7444");
}

#[test]
fn truncated_files_never_panic() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ez-prefetch/CHROME.EXE-B3BA7868.pf");
    let data = std::fs::read(p).unwrap();
    for cut in [0, 3, 8, 12, 100, 300, 1000, data.len() / 2, data.len() - 1] {
        let _ = prefetch::parse(&data[..cut]);
    }
    let unpacked = prefetch::unpack(&data).unwrap().into_owned();
    for cut in (0..unpacked.len()).step_by(97) {
        let _ = prefetch::parse(&unpacked[..cut]);
    }
    let mut flipped = data.clone();
    for i in (8..flipped.len()).step_by(13) {
        flipped[i] ^= 0x5A;
    }
    let _ = prefetch::parse(&flipped);
}
