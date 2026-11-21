//! 실제 Windows가 만든 .lnk 샘플로 하는 검증.
//!
//! 샘플 출처: github.com/AbdulRhmanAlfaifi/lnk_parser (MIT). `tests/data/lnk/LICENSE-samples` 참고.
//!
//! 1. 알려진 값 확인: 샘플 저장소 README에 적힌 값과 일치하는가
//! 2. 교차 검증: 독립적으로 만들어진 lnk_parser crate와 결과가 같은가
//! 3. 내성 시험: 자르거나 바이트를 바꿔도 패닉하지 않는가

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use st_parsers::lnk;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lnk")
}

fn all_samples() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("lnk") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&data_dir(), &mut out);
    out.sort();
    out
}

#[test]
fn known_values_from_sample_readme() {
    let data = fs::read(data_dir().join("WIN10/1607_14393/windows_generated.lnk")).unwrap();
    let info = lnk::parse(&data, encoding_rs::WINDOWS_1252).unwrap();
    assert_eq!(info.target_path().as_deref(), Some("C:\\Users\\u0041\\Desktop\\test\\test.txt"));
    assert_eq!(info.drive_type_name(), Some("fixed"));
    assert_eq!(info.drive_serial_display().as_deref(), Some("E02E-8A93"));
    assert_eq!(info.target_size, 4);
    assert_eq!(info.target_modified.unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2021-02-08T12:41:58Z");
    assert_eq!(info.target_created.unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2021-02-08T12:41:03Z");
}

#[test]
fn cross_check_against_independent_parser() {
    let samples = all_samples();
    assert!(samples.len() >= 15, "샘플이 부족함: {}", samples.len());

    let mut compared = 0;
    let mut report = Vec::new();
    for path in &samples {
        let data = fs::read(path).unwrap();
        let ours = lnk::parse(&data, encoding_rs::WINDOWS_1252);
        let theirs = lnk_parser::LNKParser::from_reader(&mut Cursor::new(&data));
        let name = path.strip_prefix(data_dir()).unwrap().display().to_string();

        let (Ok(ours), Ok(theirs)) = (ours, theirs) else {
            panic!("{name}: 두 파서 중 하나가 실패함");
        };
        let theirs = serde_json::to_value(&theirs).unwrap();

        // 대상 파일 시각·크기 (머리 부분)
        let hdr = &theirs["shell_link_header"];
        let fmt = |t: Option<chrono::DateTime<chrono::Utc>>| {
            t.map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        };
        // [MS-SHLLINK] 2.1: 시각 값이 0이면 "값이 설정되지 않음"이다. 비교 대상 파서는 이를
        // 1601-01-01로 표시하지만 우리는 None으로 둔다 (가짜 1601년 기록을 만들지 않기 위해).
        let theirs_time = |key: &str| hdr[key].as_str().filter(|s| *s != "1601-01-01T00:00:00Z");
        assert_eq!(fmt(ours.target_modified).as_deref(), theirs_time("mtime"), "{name}: mtime");
        assert_eq!(fmt(ours.target_created).as_deref(), theirs_time("ctime"), "{name}: ctime");
        assert_eq!(fmt(ours.target_accessed).as_deref(), theirs_time("atime"), "{name}: atime");
        assert_eq!(u64::from(ours.target_size), hdr["file_size"].as_u64().unwrap_or(0), "{name}: size");

        // 볼륨 정보와 경로 (LinkInfo)
        let li = &theirs["link_info"];
        // 비교 대상 파서는 앞자리 0을 생략해 표시한다("77-724"). Windows의 `vol` 명령은
        // 네 자리씩 0을 채워 보여주므로("0077-0724") 우리는 그 형태를 쓰고, 비교는 숫자로 한다.
        let their_serial = li["volume_id"]["serial_number"].as_str().map(|s| {
            let (hi, lo) = s.split_once('-').expect("시리얼 형식");
            (u32::from_str_radix(hi, 16).unwrap() << 16) | u32::from_str_radix(lo, 16).unwrap()
        });
        assert_eq!(ours.drive_serial, their_serial, "{name}: 볼륨 시리얼");
        let their_type = li["volume_id"]["drive_type"].as_str().map(|s| s.to_ascii_lowercase());
        let our_type = ours.drive_type_name().map(|s| format!("drive_{s}"));
        assert_eq!(our_type, their_type, "{name}: 드라이브 종류");
        assert_eq!(
            ours.local_base_path.as_deref(),
            li["local_base_path"].as_str(),
            "{name}: 로컬 경로"
        );

        compared += 1;
        report.push(format!(
            "{name}: type={:?} serial={:?} target={:?}",
            ours.drive_type_name(),
            ours.drive_serial_display(),
            ours.target_path()
        ));
    }
    println!("교차 검증한 샘플 {compared}개\n{}", report.join("\n"));
}

#[test]
fn network_share_sample_has_unc_target() {
    let data = fs::read(data_dir().join("WIN10/1607_14393/network_share.lnk")).unwrap();
    let info = lnk::parse(&data, encoding_rs::WINDOWS_1252).unwrap();
    let target = info.target_path().unwrap();
    assert!(target.starts_with("\\\\"), "UNC 경로가 아님: {target}");
    assert!(info.net_name.is_some());
}

#[test]
fn not_a_link_is_rejected() {
    assert_eq!(lnk::parse(b"", encoding_rs::WINDOWS_1252), Err(lnk::LnkError::NotALink));
    assert_eq!(lnk::parse(&[0u8; 200], encoding_rs::WINDOWS_1252), Err(lnk::LnkError::NotALink));
}

/// 간이 내성 시험 (정식 퍼징을 대신하지는 못한다).
#[test]
fn survives_truncation_and_byte_flips() {
    for path in all_samples() {
        let data = fs::read(&path).unwrap();
        for len in 0..data.len() {
            let _ = lnk::parse(&data[..len], encoding_rs::EUC_KR);
        }
        // 구조 정보가 몰려 있는 앞부분은 모든 바이트를, 뒷부분은 건너뛰며 바꿔 본다.
        let dense = data.len().min(600);
        for i in (0..dense).chain((dense..data.len()).step_by(7)) {
            for v in [0x00u8, 0xFF, 0x7F, 0x80] {
                let mut m = data.clone();
                m[i] = v;
                let _ = lnk::parse(&m, encoding_rs::EUC_KR);
            }
        }
    }
}
