//! 설정·고객사·지난 내역 저장.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use st_core::settings::{Customer, Settings};
use st_rules::Grade;

/// 데이터 폴더: 환경 변수 `ST_DATA_DIR`, 없으면 실행 파일 옆 `data`.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("ST_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

pub fn history_dir(data: &Path) -> PathBuf {
    data.join("history")
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("폴더를 만들지 못함: {e}"))?;
    }
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    // 저장 중 꺼져도 이전 파일이 깨지지 않도록 임시 파일에 쓴 뒤 바꾼다.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, s).map_err(|e| format!("저장하지 못함: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("저장하지 못함: {e}"))
}

pub fn load_settings(data: &Path) -> Settings {
    read_json(&data.join("settings.json"))
}

pub fn save_settings(data: &Path, s: &Settings) -> Result<(), String> {
    write_json(&data.join("settings.json"), s)
}

pub fn load_customers(data: &Path) -> Vec<Customer> {
    read_json(&data.join("customers.json"))
}

pub fn save_customers(data: &Path, c: &[Customer]) -> Result<(), String> {
    write_json(&data.join("customers.json"), &c.to_vec())
}

/// 설정과 고객사를 한 파일로 내보내기·가져오기 (다른 USB로 옮길 때).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bundle {
    pub settings: Settings,
    pub customers: Vec<Customer>,
}

pub fn export_bundle(data: &Path, to: &Path) -> Result<(), String> {
    write_json(to, &Bundle { settings: load_settings(data), customers: load_customers(data) })
}

pub fn import_bundle(data: &Path, from: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(from).map_err(|e| format!("파일을 읽지 못함: {e}"))?;
    let b: Bundle = serde_json::from_str(&text).map_err(|e| format!("형식이 올바르지 않음: {e}"))?;
    save_settings(data, &b.settings)?;
    save_customers(data, &b.customers)
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub dir: PathBuf,
    pub started: DateTime<Utc>,
    pub customer: String,
    pub pc_name: String,
    pub analyst: String,
    pub grade: Grade,
    /// 저장 후 파일이 바뀌었는가 (SHA256SUMS.txt와 비교)
    pub changed: Vec<String>,
    /// 보관 1년 경과
    pub old: bool,
    pub reports: Vec<String>,
}

/// 지난 내역 목록 (최근 것부터). 깨진 항목은 건너뛴다.
pub fn list_history(history: &Path) -> Vec<HistoryEntry> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(history) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(case) = st_report::load_case(&dir) else { continue };
        let changed = st_report::verify_sums(&dir).unwrap_or_else(|_| vec!["SHA256SUMS.txt".to_string()]);
        let mut reports: Vec<String> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|f| f.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("report_") && n.ends_with(".html"))
                    .collect()
            })
            .unwrap_or_default();
        reports.sort();
        out.push(HistoryEntry {
            dir,
            started: case.info.started_utc,
            customer: case.info.customer,
            pc_name: case.info.pc_name,
            analyst: case.info.analyst,
            grade: case.assessment.grade,
            changed,
            old: Utc::now() - case.info.started_utc > Duration::days(365),
            reports,
        });
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.started));
    out
}

/// 내역 하나를 완전히 지운다. 내역 폴더 밖의 경로는 거부한다.
pub fn delete_history(history: &Path, dir: &Path) -> Result<(), String> {
    let h = history.canonicalize().map_err(|e| e.to_string())?;
    let d = dir.canonicalize().map_err(|e| e.to_string())?;
    if !d.starts_with(&h) || d == h {
        return Err("내역 폴더 밖은 지울 수 없습니다.".to_string());
    }
    std::fs::remove_dir_all(&d).map_err(|e| format!("지우지 못함: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_safe_delete() {
        let d = std::env::temp_dir().join(format!("st-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut s = load_settings(&d);
        assert_eq!(s, Settings::default());
        s.ui_lang = "en".into();
        save_settings(&d, &s).unwrap();
        assert_eq!(load_settings(&d).ui_lang, "en");

        let c = vec![Customer { name: "고객사A".into(), ..Default::default() }];
        save_customers(&d, &c).unwrap();
        assert_eq!(load_customers(&d), c);

        let bundle = d.join("bundle.json");
        export_bundle(&d, &bundle).unwrap();
        let d2 = d.join("other");
        import_bundle(&d2, &bundle).unwrap();
        assert_eq!(load_customers(&d2), c);

        let h = history_dir(&d);
        std::fs::create_dir_all(h.join("x")).unwrap();
        assert!(delete_history(&h, &d).is_err(), "내역 폴더 밖은 거부");
        assert!(delete_history(&h, &h).is_err(), "내역 폴더 자체도 거부");
        delete_history(&h, &h.join("x")).unwrap();
        assert!(list_history(&h).is_empty());
    }
}
