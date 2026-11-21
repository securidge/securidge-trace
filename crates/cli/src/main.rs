//! trace: 명령줄 실행 파일.
//!
//! 사용 예
//! ```text
//! trace analyze --input D:\evidence\PC01 --output D:\reports --case "2026-011" --analyst "홍길동"
//! ```
//!
//! 종료 코드: 0 = 정상, 2 = 끝났지만 읽지 못한 항목이 있음, 1 = 입력 오류 등으로 실행하지 못함

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::Utc;
use chrono_tz::Tz;
use clap::{Parser, Subcommand};
use st_i18n::{t, Lang};
use st_report::RunInfo;

#[derive(Parser)]
#[command(name = "trace", version, about = "Windows 사용자 행위 흔적 점검 도구 (Securidge Trace)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 수집해 둔 자료(폴더 또는 읽기 전용으로 마운트한 이미지)를 분석한다
    Analyze {
        /// 입력 폴더 (Windows의 원래 폴더 구조를 유지한 것)
        #[arg(long)]
        input: PathBuf,
        /// 결과를 저장할 폴더 (입력 폴더 밖이어야 함)
        #[arg(long)]
        output: PathBuf,
        /// 사건명 또는 고객사·PC 이름
        #[arg(long = "case", default_value = "case")]
        case_name: String,
        /// 점검자
        #[arg(long, default_value = "")]
        analyst: String,
        /// 표시 언어: ko 또는 en
        #[arg(long, default_value = "ko")]
        lang: String,
        /// 표시 시간대 (IANA 이름)
        #[arg(long, default_value = "Asia/Seoul")]
        tz: String,
        /// 대상 PC의 ANSI 코드 페이지: 949(한국어) 또는 1252(영어·서유럽)
        #[arg(long, default_value = "949")]
        ansi_codepage: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { input, output, case_name, analyst, lang, tz, ansi_codepage } => {
            let lang = Lang::parse(&lang).unwrap_or(Lang::Ko);
            match analyze(&input, &output, &case_name, &analyst, lang, &tz, &ansi_codepage) {
                Ok(code) => code,
                Err(msg) => {
                    eprintln!("{msg}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze(
    input: &Path,
    output: &Path,
    case_name: &str,
    analyst: &str,
    lang: Lang,
    tz_name: &str,
    ansi_codepage: &str,
) -> Result<ExitCode, String> {
    let tz: Tz = tz_name.parse().map_err(|_| format!("알 수 없는 시간대: {tz_name}"))?;
    let ansi = match ansi_codepage {
        "949" => encoding_rs::EUC_KR, // encoding_rs의 EUC_KR은 Windows 코드 페이지 949와 같다
        "1252" => encoding_rs::WINDOWS_1252,
        other => return Err(format!("지원하지 않는 코드 페이지: {other} (949 또는 1252)")),
    };

    let started = Utc::now();

    // 1. 입력을 건드리지 않기 위한 확인
    if !input.is_dir() {
        return Err(t(lang, "cli.error.input_not_found"));
    }
    if st_collect::is_inside(output, input) {
        return Err(t(lang, "cli.error.output_inside_input"));
    }

    // 2. 입력 훑기
    let scan = st_collect::scan(input).map_err(|_| t(lang, "cli.error.input_not_found"))?;
    println!("[{}]", t(lang, "cli.scan.title"));
    for check in &scan.checks {
        let state = if check.count > 0 {
            format!("{} ({})", t(lang, "cli.scan.found"), check.count)
        } else {
            t(lang, "cli.scan.missing")
        };
        println!("  - {}: {state}", t(lang, &format!("artifact.{}", check.artifact)));
    }

    // 3. 출력 폴더 정하기 (실행 시각을 이름에 넣는다)
    let out_dir = unique_dir(output, &format!(
        "Trace_{}_{}",
        sanitize(case_name),
        st_i18n::format_file_stamp(started, tz)
    ));
    let work_dir = out_dir.join(".work");
    std::fs::create_dir_all(&work_dir).map_err(|e| format!("출력 폴더를 만들지 못함: {e}"))?;

    // 4. 분석 전 해시 → 분석 → 분석 후 해시
    let hashes_before = st_collect::hash_inputs(&scan);
    let analysis = st_timeline::analyze(&scan, &st_timeline::Options { work_dir: &work_dir, ansi });
    let hashes_after = st_collect::hash_inputs(&scan);
    let _ = std::fs::remove_dir_all(&work_dir);
    let input_unchanged = hashes_before == hashes_after;

    // 5. 결과 쓰기
    let finished = Utc::now();
    let run = RunInfo {
        case_name: case_name.to_string(),
        analyst: analyst.to_string(),
        lang: lang.code().to_string(),
        display_time_zone: tz.name().to_string(),
        started_utc: started,
        finished_utc: finished,
        started_display: st_i18n::format_stamp_with_zone(started, tz, lang),
        finished_display: st_i18n::format_stamp_with_zone(finished, tz, lang),
        clock_note: t(lang, "cli.run.clock_note"),
        input_root: input.display().to_string(),
        input_unchanged,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    st_report::write_all(
        &out_dir,
        &st_report::Inputs {
            analysis: &analysis,
            checks: &scan.checks,
            input_hashes: &hashes_before,
            run: &run,
            tz,
            lang,
        },
    )?;

    // 6. 요약 출력
    println!();
    println!("{}: {}", t(lang, "cli.run.started"), run.started_display);
    println!("{}: {}", t(lang, "cli.run.finished"), run.finished_display);
    println!("  ({})", run.clock_note);
    println!("{}: {}", t(lang, "cli.result.events"), analysis.events.len());
    println!("{}: {}", t(lang, "cli.result.issues"), analysis.issues.len());
    if input_unchanged {
        println!("{}", t(lang, "cli.result.input_unchanged"));
    } else {
        println!("{}", t(lang, "cli.result.input_changed"));
    }
    println!("{}: {}", t(lang, "cli.result.saved"), out_dir.display());
    println!("{}", t(lang, "cli.notice.reference_only"));

    Ok(if analysis.issues.is_empty() && input_unchanged {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

/// 파일 이름에 쓸 수 없는 글자를 '_'로 바꾼다. 한글 등 글자는 그대로 둔다.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(60)
        .collect();
    if cleaned.is_empty() {
        "case".to_string()
    } else {
        cleaned
    }
}

/// 같은 이름의 폴더가 이미 있으면 뒤에 번호를 붙인다 (지난 결과를 덮어쓰지 않는다).
fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let candidate = parent.join(format!("{name}_{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}
