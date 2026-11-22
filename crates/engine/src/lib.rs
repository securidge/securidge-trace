//! st-engine: 점검 한 건을 처음부터 끝까지 실행하고, 설정·고객사·지난 내역을 관리한다.
//! 화면(GUI)과 명령줄이 모두 이 crate만 부른다.
//!
//! 저장 위치: 실행 파일 옆 `data` 폴더 (환경 변수 `ST_DATA_DIR`로 바꿀 수 있다).
//! ```text
//! data/settings.json        기본 설정
//! data/customers.json       고객사 프로필
//! data/history/<시각>_<PC>/ 점검 한 건 (case.json, report_*.html, evidence.json, timeline.csv, SHA256SUMS.txt)
//! ```

pub mod live;
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Duration, Utc};
use chrono_tz::Tz;
use st_core::settings::{Customer, RuleConfig};
use st_i18n::Lang;
use st_report::{CaseData, CaseInfo, CollectedFile, CASE_SCHEMA_VERSION};
use st_rules::ToolMedia;

#[derive(Debug, Clone)]
pub enum Mode {
    /// 이 PC를 직접 점검 (Windows)
    Live,
    /// 수집해 둔 자료(폴더·읽기 전용 디스크) 분석
    Offline(PathBuf),
}

#[derive(Debug, Clone)]
pub struct CheckParams {
    pub mode: Mode,
    pub customer: Customer,
    pub pc_name: String,
    pub analyst: String,
    pub company: String,
    pub period_days: u32,
    pub report_lang: Lang,
    pub time_zone: String,
    pub consent: bool,
    pub mask_names: bool,
    pub keep_copies: bool,
    pub rules: RuleConfig,
    /// 내역을 저장할 폴더 (보통 data/history)
    pub history_dir: PathBuf,
    /// 유니코드 경로가 없는 바로 가기를 풀 때 쓸 코드 페이지 (레지스트리에서 못 읽었을 때)
    pub fallback_codepage: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Collect,
    Analyze,
    Report,
    Done,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub dir: PathBuf,
    pub case: CaseData,
    pub report_path: PathBuf,
    pub warnings: Vec<String>,
}

pub type Progress<'a> = &'a (dyn Fn(Stage, &str) + Sync);

fn cancelled(c: &AtomicBool) -> Result<(), String> {
    if c.load(Ordering::Relaxed) {
        Err("취소했습니다.".to_string())
    } else {
        Ok(())
    }
}

fn sanitize(s: &str) -> String {
    let cleaned: String =
        s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    if cleaned.is_empty() {
        "PC".to_string()
    } else {
        cleaned
    }
}

/// 점검 한 건을 실행한다.
pub fn run_check(p: &CheckParams, progress: Progress, cancel: &AtomicBool) -> Result<CheckResult, String> {
    if !p.consent {
        return Err("점검 권한 확인이 필요합니다.".to_string());
    }
    let tz: Tz = p.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
    let started = Utc::now();
    let mut warnings = Vec::new();

    let stamp = st_i18n::format_file_stamp(started, tz);
    let dir = unique_dir(&p.history_dir, &format!("{stamp}_{}", sanitize(&p.pc_name)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("저장 폴더를 만들지 못함: {e}"))?;
    let work = dir.join(".work");
    std::fs::create_dir_all(&work).map_err(|e| format!("작업 폴더를 만들지 못함: {e}"))?;

    let result = run_inner(p, progress, cancel, &dir, &work, started, tz, &mut warnings);
    let _ = std::fs::remove_dir_all(&work);
    match result {
        Ok((case, report_path)) => {
            progress(Stage::Done, "");
            Ok(CheckResult { dir, case, report_path, warnings })
        }
        Err(e) => {
            // 실패한 점검의 빈 폴더는 남기지 않는다.
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    p: &CheckParams,
    progress: Progress,
    cancel: &AtomicBool,
    dir: &Path,
    work: &Path,
    started: chrono::DateTime<Utc>,
    tz: Tz,
    warnings: &mut Vec<String>,
) -> Result<(CaseData, PathBuf), String> {
    // --- 1. 자료 모으기 ---
    progress(Stage::Collect, "");
    let mut collection: Vec<CollectedFile> = Vec::new();
    let mut tool_media = ToolMedia::default();
    let mut admin = true;
    let mut drives: Vec<st_core::DriveInfo> = Vec::new();
    let (root, overlay, mode_name) = match &p.mode {
        Mode::Offline(path) => {
            if !path.is_dir() {
                return Err("자료 폴더를 찾을 수 없습니다.".to_string());
            }
            if st_collect::is_inside(dir, path) {
                return Err("저장 폴더가 자료 폴더 안에 있습니다. 다른 위치를 쓰세요.".to_string());
            }
            (path.clone(), None, "offline")
        }
        Mode::Live => {
            if !cfg!(windows) {
                return Err("이 PC 점검은 Windows에서만 할 수 있습니다.".to_string());
            }
            admin = live::is_admin();
            let root = live::system_root();
            if st_collect::is_inside(dir, &root) {
                warnings.push("결과가 점검 대상 PC의 디스크에 저장됩니다. USB 등 다른 저장장치에서 실행하는 것을 권합니다.".to_string());
            }
            tool_media = live::tool_media(&root);
            drives = live::drives();
            let overlay = work.join("collected");
            if admin {
                collection = live::collect(&overlay, &|msg| progress(Stage::Collect, msg), cancel)?;
            } else {
                warnings.push("관리자 권한이 없어 장치·네트워크·이벤트 로그를 읽지 못했습니다.".to_string());
            }
            (root, Some(overlay), "live")
        }
    };
    cancelled(cancel)?;

    // --- 2. 기록 읽기 ---
    progress(Stage::Analyze, "");
    let scan = st_collect::scan_with_overlay(&root, overlay.as_deref().filter(|o| o.is_dir()))
        .map_err(|_| "자료 폴더를 찾을 수 없습니다.".to_string())?;
    let hashes_before = st_collect::hash_inputs(&scan);
    cancelled(cancel)?;
    let fallback = match p.fallback_codepage {
        1252 => encoding_rs::WINDOWS_1252,
        _ => encoding_rs::EUC_KR,
    };
    let analysis = st_timeline::analyze(&scan, &st_timeline::Options { work_dir: work, fallback_ansi: fallback, drives: &drives });
    cancelled(cancel)?;
    let hashes_after = st_collect::hash_inputs(&scan);
    let input_unchanged = hashes_before == hashes_after;

    let period_start = (p.period_days > 0).then(|| started - Duration::days(i64::from(p.period_days)));
    let assessment = st_rules::assess(
        &analysis,
        &st_rules::Context {
            rules: &p.rules,
            customer: &p.customer,
            tz,
            period_start,
            period_end: Some(started),
            tool_media: &tool_media,
            drives: &drives,
        },
    );

    // --- 3. 보고서 ---
    progress(Stage::Report, "");
    let pc_name = if p.pc_name.trim().is_empty() {
        analysis.system.computer_name.clone().unwrap_or_default()
    } else {
        p.pc_name.clone()
    };
    let case = CaseData {
        schema_version: CASE_SCHEMA_VERSION,
        info: CaseInfo {
            customer: p.customer.name.clone(),
            pc_name,
            analyst: p.analyst.clone(),
            company: p.company.clone(),
            mode: mode_name.to_string(),
            input_root: root.display().to_string(),
            started_utc: started,
            finished_utc: Utc::now(),
            period_days: p.period_days,
            consent: p.consent,
            admin,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            tool_media,
            input_unchanged,
            time_zone: tz.name().to_string(),
            mask_names: p.mask_names,
            drives,
        },
        checks: scan.checks.clone(),
        input_hashes: hashes_before,
        collection,
        analysis,
        assessment,
    };
    if p.keep_copies {
        if let Some(o) = overlay.as_ref().filter(|o| o.is_dir()) {
            let _ = std::fs::rename(o, dir.join("collected"));
        }
    }
    // 두 언어 보고서를 함께 만든다. 고른 언어가 먼저(CSV 언어).
    let other = if p.report_lang == Lang::Ko { Lang::En } else { Lang::Ko };
    st_report::write_all(dir, &case, &[p.report_lang, other])?;
    let report_path = dir.join(format!("report_{}.html", p.report_lang.code()));
    Ok((case, report_path))
}

fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let c = parent.join(format!("{name}_{n}"));
        if !c.exists() {
            return c;
        }
    }
    first
}

/// 파일·폴더를 Windows 기본 프로그램으로 연다 (보고서는 브라우저, 폴더는 탐색기).
pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("explorer")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("열지 못함: {e}"))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ()).map_err(|e| format!("열지 못함: {e}"))
    }
}
