//! GitHub Actions의 Windows 가상 머신(Windows Server 2022, 2025)이 실제로 만든 파일로 하는 회귀 시험.
//!
//! 이 파일들은 `ci/win-setup-truth.ps1`이 만든 것이며, 각 폴더의 `truth.json`에 정답
//! (대상 경로, 크기, 볼륨 시리얼, 만든 시각 범위)이 함께 들어 있다.
//! - `$I*` : Windows가 휴지통으로 보낼 때 직접 만든 파일
//! - `opened_*.lnk` : Windows 셸의 IShellLinkW로 만든 바로 가기
//!
//! 한계: 서버용 Windows다. 클라이언트 Windows 10/11, 실제 USB의 바로 가기는 아직 없다.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use st_parsers::{lnk, recyclebin};

fn dirs() -> Vec<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/windows-gha");
    let mut v: Vec<PathBuf> =
        fs::read_dir(base).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

fn truth(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(dir.join("truth.json")).unwrap()).unwrap()
}

fn ts(v: &serde_json::Value) -> DateTime<Utc> {
    // PowerShell 'o' 형식은 소수점 7자리다. chrono는 그대로 읽는다.
    DateTime::parse_from_rfc3339(v.as_str().unwrap()).unwrap().with_timezone(&Utc)
}

#[test]
fn real_windows_recycle_bin_files_match_truth() {
    let mut checked = 0;
    for dir in dirs() {
        let t = truth(&dir);
        let entries: Vec<recyclebin::RecycleEntry> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("$I"))
            .map(|p| recyclebin::parse_index(&fs::read(&p).unwrap()).unwrap())
            .collect();
        for d in t["deleted"].as_array().unwrap() {
            let path = d["original_path"].as_str().unwrap();
            let e = entries
                .iter()
                .find(|e| e.original_path == path)
                .unwrap_or_else(|| panic!("{}: {path} 없음", dir.display()));
            // Windows 10 이후 형식은 버전 2
            assert_eq!(e.version, 2);
            assert_eq!(e.original_size, d["size"].as_u64().unwrap());
            let when = e.deleted_at.unwrap();
            let lo = ts(&d["time_min_utc"]) - chrono::Duration::seconds(1);
            let hi = ts(&d["time_max_utc"]) + chrono::Duration::seconds(1);
            assert!(lo <= when && when <= hi, "{path}: 삭제 시각 {when}이 {lo}~{hi} 밖");
            checked += 1;
        }
    }
    assert!(checked >= 4, "검사한 휴지통 기록 {checked}개");
}

#[test]
fn real_windows_shortcuts_match_truth() {
    let mut checked = 0;
    for dir in dirs() {
        let t = truth(&dir);
        for o in t["opened"].as_array().unwrap() {
            let label = o["label"].as_str().unwrap();
            let data = fs::read(dir.join(format!("{label}.lnk"))).unwrap();
            let info = lnk::parse(&data, encoding_rs::EUC_KR).unwrap();
            assert_eq!(info.target_path().as_deref(), o["target"].as_str(), "{label}");
            assert_eq!(info.drive_serial_display().as_deref(), o["volume_serial"].as_str(), "{label}");
            assert_eq!(u64::from(info.target_size), o["size"].as_u64().unwrap(), "{label}");
            assert_eq!(info.drive_type_name(), Some("fixed"), "{label}: VHD도 고정 디스크로 보인다");
            checked += 1;
        }
    }
    assert!(checked >= 6, "검사한 바로 가기 {checked}개");
}
