//! st-report: 분석 결과를 파일로 내보낸다.
//!
//! 지금 만드는 파일
//! - `evidence.json` : 근거 데이터 전체. **같은 입력이면 항상 같은 내용**이 나오도록
//!   실행 시각이나 절대 경로 같은 값은 넣지 않는다 (개발 요청서 2장 8번: 재현 가능).
//! - `timeline.csv`  : 사람이 엑셀로 열어 볼 수 있는 시간순 목록.
//! - `run.json`      : 이번 실행에 대한 정보 (실행 시각, 점검자 등). 실행마다 달라진다.
//! - `SHA256SUMS.txt`: 위 파일들의 해시.
//!
//! 2단 HTML 보고서는 Phase 4 과제다.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use st_collect::ScanCheck;
use st_core::{hash, Coverage, Event, Issue};
use st_i18n::Lang;
use st_timeline::Analysis;

/// 근거 파일 형식 버전. 구조를 바꾸면 올린다 (예전 내역을 열 수 있게 하기 위함).
pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    tool_version: &'a str,
    checks: &'a [ScanCheck],
    coverage: &'a [Coverage],
    input_hashes: &'a BTreeMap<String, String>,
    events: &'a [Event],
    issues: &'a [Issue],
}

/// 이번 실행에 대한 정보.
#[derive(Debug, Clone, Serialize)]
pub struct RunInfo {
    pub case_name: String,
    pub analyst: String,
    pub lang: String,
    pub display_time_zone: String,
    pub started_utc: DateTime<Utc>,
    pub finished_utc: DateTime<Utc>,
    /// 표시용 시각 (개발 요청서 9.1 형식)
    pub started_display: String,
    pub finished_display: String,
    pub clock_note: String,
    pub input_root: String,
    /// 분석 전후 입력 해시가 같았는가
    pub input_unchanged: bool,
    pub tool_version: String,
}

pub struct Inputs<'a> {
    pub analysis: &'a Analysis,
    pub checks: &'a [ScanCheck],
    pub input_hashes: &'a BTreeMap<String, String>,
    pub run: &'a RunInfo,
    pub tz: Tz,
    pub lang: Lang,
}

/// 결과 파일들을 `dir`에 쓴다. 쓴 파일 이름 → SHA-256을 돌려준다.
pub fn write_all(dir: &Path, inp: &Inputs) -> Result<BTreeMap<String, String>, String> {
    fs::create_dir_all(dir).map_err(|e| format!("출력 폴더를 만들지 못함: {e}"))?;

    let evidence = Evidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        tool_version: &inp.run.tool_version,
        checks: inp.checks,
        coverage: &inp.analysis.coverage,
        input_hashes: inp.input_hashes,
        events: &inp.analysis.events,
        issues: &inp.analysis.issues,
    };
    let evidence_json =
        serde_json::to_vec_pretty(&evidence).map_err(|e| format!("근거 JSON 생성 실패: {e}"))?;
    write_file(&dir.join("evidence.json"), &evidence_json)?;

    write_file(&dir.join("timeline.csv"), &timeline_csv(&inp.analysis.events, inp.tz, inp.lang)?)?;

    let run_json =
        serde_json::to_vec_pretty(inp.run).map_err(|e| format!("실행 정보 생성 실패: {e}"))?;
    write_file(&dir.join("run.json"), &run_json)?;

    let mut sums = BTreeMap::new();
    for name in ["evidence.json", "timeline.csv", "run.json"] {
        let h = hash::sha256_file(&dir.join(name)).map_err(|e| format!("해시 계산 실패: {e}"))?;
        sums.insert(name.to_string(), h);
    }
    let mut listing = String::new();
    for (name, h) in &sums {
        listing.push_str(&format!("{h}  {name}\n"));
    }
    write_file(&dir.join("SHA256SUMS.txt"), listing.as_bytes())?;
    Ok(sums)
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
            "time_display",
            "time_utc",
            "time_basis",
            "kind",
            "kind_label",
            "subject",
            "account_name",
            "account_sid",
            "confidence",
            "artifact",
            "source_file",
            "source_record",
            "details",
            "caveats",
        ])
        .map_err(|e| e.to_string())?;
        for e in events {
            let display = e
                .time
                .utc
                .map(|t| st_i18n::format_stamp_with_zone(t, tz, lang))
                .unwrap_or_default();
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
                e.account.name.as_deref().unwrap_or(""),
                e.account.sid.as_deref().unwrap_or(""),
                &format!("{:?}", e.confidence),
                e.source.artifact.as_str(),
                e.source.file.as_str(),
                e.source.record.as_str(),
                details.as_str(),
                &e.caveats.join(" / "),
            ])
            .map_err(|e| e.to_string())?;
        }
        w.flush().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// 엑셀은 `=`, `+`, `-`, `@`로 시작하는 칸을 수식으로 실행한다. 분석 대상에서 온 값
/// (파일 이름, URL)이 그렇게 시작하면 앞에 작은따옴표를 붙여 글자로만 읽히게 한다.
fn neutralize_formula(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_like_values_are_neutralized() {
        assert_eq!(neutralize_formula("=cmd|' /C calc'!A0"), "'=cmd|' /C calc'!A0");
        assert_eq!(neutralize_formula("C:\\a.txt"), "C:\\a.txt");
        assert_eq!(neutralize_formula(""), "");
    }
}
