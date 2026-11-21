//! trace: 명령줄 실행 파일 (자동화·전문가·테스트용). 화면 프로그램과 같은 엔진을 쓴다.
//!
//! ```text
//! trace check   --customer "고객사A" --analyst "홍길동" --i-am-authorized   이 PC 점검 (Windows, 관리자 권한)
//! trace analyze --input E:\ --output D:\reports --case "PC-01"               수집해 둔 자료 분석
//! trace regen   --dir <내역 폴더> --lang en                                   다른 언어로 보고서 다시 만들기
//! ```
//! 종료 코드: 0 = 정상, 2 = 끝났지만 읽지 못한 항목이 있거나 입력이 바뀜, 1 = 실행하지 못함

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use clap::{Parser, Subcommand};
use st_core::settings::Customer;
use st_engine::{CheckParams, Mode, Stage};
use st_i18n::{t, Lang};

#[derive(Parser)]
#[command(name = "trace", version, about = "Securidge Trace — Windows PC 사용 기록 점검 도구 (명령줄)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone)]
struct Common {
    /// 결과를 저장할 폴더 (기본: 실행 파일 옆 data\history)
    #[arg(long)]
    output: Option<PathBuf>,
    /// 사건명 또는 PC 이름 (기본: 대상 PC의 컴퓨터 이름)
    #[arg(long = "case", default_value = "")]
    case_name: String,
    /// 고객사 이름 (data\customers.json에 있으면 업무 시간·회사 장치 목록을 쓴다)
    #[arg(long, default_value = "")]
    customer: String,
    /// 점검자
    #[arg(long, default_value = "")]
    analyst: String,
    /// 보고서 언어: ko 또는 en
    #[arg(long, default_value = "ko")]
    lang: String,
    /// 표시 시간대 (IANA 이름)
    #[arg(long, default_value = "Asia/Seoul")]
    tz: String,
    /// 점검 기간(일). 0이면 전체.
    #[arg(long, default_value_t = 0)]
    days: u32,
    /// 레지스트리에서 코드 페이지를 못 읽었을 때 쓸 값: 949 또는 1252
    #[arg(long, default_value_t = 949)]
    ansi_codepage: u32,
    /// 이 PC를 점검할 권한이 있음을 확인한다 (check에 필수)
    #[arg(long, default_value_t = false)]
    i_am_authorized: bool,
}

#[derive(Subcommand)]
enum Command {
    /// 이 PC를 점검한다 (Windows, 관리자 권한 권장)
    Check {
        #[command(flatten)]
        common: Common,
    },
    /// 수집해 둔 자료(폴더 또는 읽기 전용으로 연결한 디스크)를 분석한다
    Analyze {
        /// 자료 폴더 (Windows 폴더 구조를 유지한 것)
        #[arg(long)]
        input: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    /// 저장된 점검 결과로 다른 언어 보고서를 다시 만든다
    Regen {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value = "en")]
        lang: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Check { common } => run(Mode::Live, common),
        Command::Analyze { input, common } => run(Mode::Offline(input), common),
        Command::Regen { dir, lang } => {
            let lang = Lang::parse(&lang).unwrap_or(Lang::En);
            st_report::regenerate(&dir, lang).map(|p| {
                println!("{}", p.display());
                ExitCode::SUCCESS
            })
        }
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(1)
        }
    }
}

fn run(mode: Mode, c: Common) -> Result<ExitCode, String> {
    let lang = Lang::parse(&c.lang).unwrap_or(Lang::Ko);
    // '수집해 둔 자료 분석'은 이미 확보된 자료라 별도 확인 없이 허용한다.
    let consent = c.i_am_authorized || matches!(mode, Mode::Offline(_));
    if !consent {
        return Err("이 PC를 점검할 권한이 있으면 --i-am-authorized 를 붙여 다시 실행하세요.".to_string());
    }
    let data = st_engine::store::data_dir();
    let settings = st_engine::store::load_settings(&data);
    let customer = st_engine::store::load_customers(&data)
        .into_iter()
        .find(|x| x.name == c.customer)
        .unwrap_or_else(|| Customer { name: c.customer.clone(), ..Default::default() });
    let pc_name = if c.case_name.is_empty() && matches!(mode, Mode::Live) {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    } else {
        c.case_name.clone()
    };
    let params = CheckParams {
        mode,
        customer,
        pc_name,
        analyst: c.analyst,
        company: settings.company_name.clone(),
        period_days: c.days,
        report_lang: lang,
        time_zone: c.tz,
        consent,
        mask_names: settings.mask_names,
        keep_copies: settings.keep_collected_copies,
        rules: settings.rules.clone(),
        history_dir: c.output.unwrap_or_else(|| st_engine::store::history_dir(&data)),
        fallback_codepage: c.ansi_codepage,
    };
    let progress = |stage: Stage, msg: &str| {
        let key = match stage {
            Stage::Collect => "ui.progress.collect",
            Stage::Analyze => "ui.progress.analyze",
            Stage::Report => "ui.progress.report",
            Stage::Done => "ui.progress.done",
        };
        if msg.is_empty() {
            println!("· {}", t(lang, key));
        } else {
            println!("  {msg}");
        }
    };
    let cancel = AtomicBool::new(false);
    let r = st_engine::run_check(&params, &progress, &cancel)?;

    let info = &r.case.info;
    let failures = r.case.analysis.issues.iter().filter(|i| !i.is_note()).count();
    println!();
    println!("[{}]", t(lang, "cli.scan.title"));
    for ch in &r.case.checks {
        let state = if ch.count > 0 {
            format!("{} ({})", t(lang, "cli.scan.found"), ch.count)
        } else {
            t(lang, "cli.scan.missing")
        };
        println!("  - {}: {state}", t(lang, &format!("artifact.{}", ch.artifact)));
    }
    let tz: chrono_tz::Tz = info.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
    println!();
    println!("{}: {}", t(lang, "cli.run.started"), st_i18n::format_stamp_with_zone(info.started_utc, tz, lang));
    println!("{}: {}", t(lang, "cli.run.finished"), st_i18n::format_stamp_with_zone(info.finished_utc, tz, lang));
    println!("  ({})", t(lang, "cli.run.clock_note"));
    println!("{}: {}", t(lang, "cli.result.grade"), t(lang, r.case.assessment.grade.key()));
    println!("{}: {}", t(lang, "cli.result.events"), r.case.analysis.events.len());
    println!("{}: {failures}", t(lang, "cli.result.issues"));
    println!(
        "{}",
        t(lang, if info.input_unchanged { "cli.result.input_unchanged" } else { "cli.result.input_changed" })
    );
    for w in &r.warnings {
        println!("! {w}");
    }
    println!("{}: {}", t(lang, "cli.result.saved"), r.dir.display());
    println!("{}", t(lang, "cli.notice.reference_only"));

    Ok(if failures == 0 && info.input_unchanged { ExitCode::SUCCESS } else { ExitCode::from(2) })
}
