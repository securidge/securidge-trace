//! st-report: 점검 결과를 파일로 내보낸다.
//!
//! 만드는 파일
//! - `case.json`      : 점검 한 건의 모든 정보. 지난 내역과 "다른 언어로 다시 만들기"는 이 파일만 읽는다.
//! - `evidence.json`  : 근거 데이터. **같은 입력이면 항상 같은 내용** (실행 시각·절대 경로 없음).
//! - `timeline.csv`   : 시간순 목록 (엑셀용).
//! - `report_ko.html` / `report_en.html` : 2단 보고서 (1부 요약, 2부 근거). 외부 파일 없이 열린다.
//! - `SHA256SUMS.txt` : 위 파일들의 변조 확인 값.

pub mod html;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use st_collect::ScanCheck;
use st_core::{hash, Event};
use st_i18n::Lang;
use st_rules::{Assessment, ToolMedia};
use st_timeline::Analysis;

/// case.json 형식 버전. 구조를 바꾸면 올리고, 예전 내역을 읽을 수 있게 변환 코드를 둔다.
pub const CASE_SCHEMA_VERSION: u32 = 1;
pub const EVIDENCE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseInfo {
    pub customer: String,
    pub pc_name: String,
    pub analyst: String,
    pub company: String,
    /// "live"(이 PC 직접 점검) 또는 "offline"(수집해 둔 자료 분석)
    pub mode: String,
    pub input_root: String,
    pub started_utc: DateTime<Utc>,
    pub finished_utc: DateTime<Utc>,
    /// 점검 기간(일). 0이면 전체.
    pub period_days: u32,
    pub consent: bool,
    pub admin: bool,
    pub tool_version: String,
    pub tool_media: ToolMedia,
    pub input_unchanged: bool,
    pub time_zone: String,
    pub mask_names: bool,
    /// 점검 시점에 연결돼 있던 드라이브 (실행 중인 PC 점검일 때)
    #[serde(default)]
    pub drives: Vec<st_core::DriveInfo>,
}

/// 수집 단계에서 만든 사본 하나 (실행 중 PC 점검).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedFile {
    pub artifact: String,
    /// 읽은 방법 (예: "reg save HKLM\\SYSTEM", "wevtutil epl Security")
    pub method: String,
    pub ok: bool,
    pub sha256: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseData {
    pub schema_version: u32,
    pub info: CaseInfo,
    pub checks: Vec<ScanCheck>,
    pub input_hashes: BTreeMap<String, String>,
    pub collection: Vec<CollectedFile>,
    pub analysis: Analysis,
    pub assessment: Assessment,
}

#[derive(Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    tool_version: &'a str,
    checks: &'a [ScanCheck],
    coverage: &'a [st_core::Coverage],
    input_hashes: &'a BTreeMap<String, String>,
    system: &'a st_parsers::registry::SystemInfo,
    accounts: &'a [st_parsers::registry::Account],
    devices: &'a [st_parsers::registry::Device],
    networks: &'a [st_parsers::registry::Network],
    events: &'a [Event],
    issues: &'a [st_core::Issue],
}

/// 결과 파일들을 `dir`에 쓴다. 쓴 파일 이름 → SHA-256을 돌려준다.
pub fn write_all(dir: &Path, case: &CaseData, report_langs: &[Lang]) -> Result<BTreeMap<String, String>, String> {
    fs::create_dir_all(dir).map_err(|e| format!("출력 폴더를 만들지 못함: {e}"))?;
    let tz: Tz = case.info.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);

    let a = &case.analysis;
    let evidence = Evidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        tool_version: &case.info.tool_version,
        checks: &case.checks,
        coverage: &a.coverage,
        input_hashes: &case.input_hashes,
        system: &a.system,
        accounts: &a.accounts,
        devices: &a.devices,
        networks: &a.networks,
        events: &a.events,
        issues: &a.issues,
    };
    write_json(&dir.join("evidence.json"), &evidence)?;
    write_json(&dir.join("case.json"), case)?;
    let csv_lang = report_langs.first().copied().unwrap_or(Lang::Ko);
    write_file(&dir.join("timeline.csv"), &timeline_csv(&a.events, tz, csv_lang)?)?;
    for &lang in report_langs {
        let html = html::render(case, lang, None);
        write_file(&dir.join(format!("report_{}.html", lang.code())), html.as_bytes())?;
    }
    write_sums(dir)
}

/// 저장된 case.json으로 다른 언어 보고서를 다시 만든다 (재수집 없음).
pub fn regenerate(dir: &Path, lang: Lang) -> Result<std::path::PathBuf, String> {
    let case = load_case(dir)?;
    let html = html::render(&case, lang, Some(Utc::now()));
    let path = dir.join(format!("report_{}.html", lang.code()));
    write_file(&path, html.as_bytes())?;
    write_sums(dir)?;
    Ok(path)
}

pub fn load_case(dir: &Path) -> Result<CaseData, String> {
    let text = fs::read_to_string(dir.join("case.json")).map_err(|e| format!("case.json을 읽지 못함: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("case.json 형식 오류: {e}"))
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(v).map_err(|e| format!("JSON 생성 실패: {e}"))?;
    write_file(path, &data)
}

/// 폴더 안 결과 파일들의 SHA-256 목록을 다시 쓴다.
pub fn write_sums(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut sums = BTreeMap::new();
    let rd = fs::read_dir(dir).map_err(|e| format!("폴더를 읽지 못함: {e}"))?;
    for entry in rd.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !p.is_file() || name == "SHA256SUMS.txt" {
            continue;
        }
        let h = hash::sha256_file(&p).map_err(|e| format!("해시 계산 실패: {e}"))?;
        sums.insert(name, h);
    }
    let mut listing = String::new();
    for (name, h) in &sums {
        listing.push_str(&format!("{h}  {name}\n"));
    }
    write_file(&dir.join("SHA256SUMS.txt"), listing.as_bytes())?;
    Ok(sums)
}

/// SHA256SUMS.txt와 실제 파일을 비교한다. 바뀐 파일 이름 목록 (없으면 빈 목록).
pub fn verify_sums(dir: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(dir.join("SHA256SUMS.txt")).map_err(|e| format!("목록을 읽지 못함: {e}"))?;
    let mut changed = Vec::new();
    for line in text.lines() {
        let Some((h, name)) = line.split_once("  ") else { continue };
        let now = hash::sha256_file(&dir.join(name)).unwrap_or_default();
        if now != h {
            changed.push(name.to_string());
        }
    }
    Ok(changed)
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut f = fs::File::create(path).map_err(|e| format!("{} 생성 실패: {e}", path.display()))?;
    f.write_all(data).map_err(|e| format!("{} 쓰기 실패: {e}", path.display()))
}

/// 시간순 목록 CSV. 엑셀이 한글을 깨뜨리지 않도록 맨 앞에 UTF-8 BOM을 넣는다.
pub fn timeline_csv(events: &[Event], tz: Tz, lang: Lang) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
    {
        let mut w = csv::Writer::from_writer(&mut out);
        w.write_record([
            "time_display", "time_utc", "time_basis", "kind", "kind_label", "subject", "account_name",
            "account_sid", "confidence", "artifact", "source_file", "source_record", "details", "caveats",
        ])
        .map_err(|e| e.to_string())?;
        for e in events {
            let display = e.time.utc.map(|t| st_i18n::format_stamp_with_zone(t, tz, lang)).unwrap_or_default();
            let utc = e.time.utc.map(|t| t.to_rfc3339()).unwrap_or_default();
            let kind = format!("{:?}", e.kind);
            let details = serde_json::to_string(&e.details).map_err(|e| e.to_string())?;
            w.write_record([
                display.as_str(),
                utc.as_str(),
                &format!("{:?}", e.time.basis),
                kind.as_str(),
                &st_i18n::t(lang, &format!("kind.{kind}")),
                neutralize_formula(&e.subject).as_str(),
                neutralize_formula(e.account.name.as_deref().unwrap_or("")).as_str(),
                e.account.sid.as_deref().unwrap_or(""),
                &format!("{:?}", e.confidence),
                e.source.artifact.as_str(),
                e.source.file.as_str(),
                e.source.record.as_str(),
                neutralize_formula(&details).as_str(),
                &e.caveats.join(" / "),
            ])
            .map_err(|e| e.to_string())?;
        }
        w.flush().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// 엑셀은 `=`, `+`, `-`, `@`로 시작하는 칸을 수식으로 실행한다. 분석 대상에서 온 값이
/// 그렇게 시작하면 앞에 작은따옴표를 붙여 글자로만 읽히게 한다.
fn neutralize_formula(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn formula_like_values_are_neutralized() {
        assert_eq!(neutralize_formula("=cmd|' /C calc'!A0"), "'=cmd|' /C calc'!A0");
        assert_eq!(neutralize_formula("C:\\a.txt"), "C:\\a.txt");
        assert_eq!(neutralize_formula(""), "");
    }
}
