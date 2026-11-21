//! 2단 HTML 보고서.
//!
//! - 1부(요약): 대표·관리자용. 신호등 등급, 주요 내용 3~5개(쉬운 문장), 다음에 할 일, "확인하지 못하는 것".
//!   전문 용어를 쓰지 않는다.
//! - 2부(근거): 보안 책임자·전문가용. 판단 근거, 이동식 저장장치별 정리, 시간순 기록, 자료별 범위,
//!   장치·네트워크·계정, 변조 확인 값, 읽지 못한 항목, 용어 풀이, 한계.
//!
//! 외부 파일(글꼴, 스크립트, 이미지)을 쓰지 않는 단일 HTML이다. 모든 값은 HTML 이스케이프한다.

use std::collections::BTreeMap;
use std::fmt::Write;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use st_core::Event;
use st_i18n::{format_stamp, format_stamp_with_zone, t, tf, Lang};
use st_rules::{Finding, Grade, Level};

use crate::CaseData;

/// 보고서에 싣는 시간순 기록의 최대 건수 (전체는 timeline.csv).
pub const TIMELINE_LIMIT: usize = 500;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

struct Ctx<'a> {
    lang: Lang,
    tz: Tz,
    mask: bool,
    case: &'a CaseData,
}

impl Ctx<'_> {
    fn t(&self, k: &str) -> String {
        t(self.lang, k)
    }
    fn when(&self, d: Option<DateTime<Utc>>) -> String {
        d.map(|d| format_stamp(d, self.tz, self.lang)).unwrap_or_else(|| "-".to_string())
    }
    fn when_str(&self, iso: &str) -> String {
        DateTime::parse_from_rfc3339(iso)
            .ok()
            .map(|d| format_stamp(d.with_timezone(&Utc), self.tz, self.lang))
            .unwrap_or_else(|| iso.to_string())
    }
    /// 가림 처리: 파일 이름과 계정 이름을 감춘다 (폴더 구조와 확장자는 남긴다).
    fn subject(&self, s: &str) -> String {
        if !self.mask {
            return s.to_string();
        }
        if s.starts_with("http") {
            return s.split('/').take(3).collect::<Vec<_>>().join("/") + "/…";
        }
        match s.rsplit_once(['\\', '/']) {
            Some((dir, file)) => {
                let ext = file.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
                let dir = mask_user_dir(dir);
                format!("{dir}\\***{ext}")
            }
            None => s.to_string(),
        }
    }
    fn account(&self, s: &str) -> String {
        if self.mask && !s.is_empty() {
            "***".to_string()
        } else {
            s.to_string()
        }
    }
}

fn mask_user_dir(dir: &str) -> String {
    let parts: Vec<&str> = dir.split('\\').collect();
    let mut out = Vec::new();
    let mut prev_users = false;
    for p in parts {
        if prev_users {
            out.push("***");
        } else {
            out.push(p);
        }
        prev_users = p.eq_ignore_ascii_case("Users");
    }
    out.join("\\")
}

/// 판단 하나를 쉬운 문장으로.
pub fn finding_sentence(f: &Finding, lang: Lang, tz: Tz) -> String {
    let p = &f.params;
    let g = |k: &str| p.get(k).cloned().unwrap_or_default();
    let time = DateTime::parse_from_rfc3339(&g("time"))
        .ok()
        .map(|d| format_stamp(d.with_timezone(&Utc), tz, lang))
        .unwrap_or_default();
    let device = g("device");
    let device_part = if device.trim().is_empty() { String::new() } else { format!(" ({device})") };
    let account = g("account");
    let account_part = if account.trim().is_empty() {
        String::new()
    } else {
        match lang {
            Lang::Ko => format!(" ({account})"),
            Lang::En => format!(" ({account})"),
        }
    };
    tf(
        lang,
        &format!("finding.{}.sentence", f.rule_id),
        &[
            ("time", &time),
            ("device", &device),
            ("device_part", &device_part),
            ("count", &g("count")),
            ("minutes", &g("minutes")),
            ("log", &g("log")),
            ("account_part", &account_part),
            ("sites", &g("sites")),
            ("program", &g("program")),
            ("ip", &g("ip")),
            ("size", &g("size")),
            ("day", &g("day")),
        ],
    )
}

const CSS: &str = r#"
:root{--ink:#1d2433;--muted:#5b6475;--line:#d9dee7;--bg:#f6f7f9;--ok:#1f8a4c;--warn:#c98a00;--bad:#c62f2f;--na:#6b7280}
*{box-sizing:border-box}body{margin:0;font-family:"Malgun Gothic","맑은 고딕","Apple SD Gothic Neo","Noto Sans KR",sans-serif;color:var(--ink);background:var(--bg);line-height:1.55;font-size:14px}
.page{max-width:1000px;margin:0 auto;background:#fff;padding:36px 44px}
h1{font-size:24px;margin:0 0 4px}h2{font-size:19px;margin:34px 0 12px;padding-bottom:6px;border-bottom:2px solid var(--ink)}h3{font-size:15px;margin:22px 0 8px}
.sub{color:var(--muted);margin:0 0 18px}
table{width:100%;border-collapse:collapse;margin:6px 0 10px;font-size:12.5px}th,td{border:1px solid var(--line);padding:5px 7px;text-align:left;vertical-align:top;word-break:break-all}th{background:#eef1f5;font-weight:600}
.meta td:first-child{width:24%;background:#fafbfc;font-weight:600;word-break:keep-all}
.grade{display:flex;align-items:center;gap:16px;border:1px solid var(--line);border-radius:10px;padding:16px 20px;margin:16px 0}
.light{width:46px;height:46px;border-radius:50%;flex:none}.g-nothing .light{background:var(--ok)}.g-caution .light{background:var(--warn)}.g-review .light{background:var(--bad)}.g-undetermined .light{background:var(--na)}
.grade .label{font-size:21px;font-weight:700}.grade .desc{color:var(--muted)}
ol.key li{margin:6px 0}.lv{display:inline-block;font-size:11px;padding:1px 7px;border-radius:9px;color:#fff;margin-right:6px;vertical-align:1px}.lv-High{background:var(--bad)}.lv-Medium{background:var(--warn)}.lv-Low{background:var(--na)}
.box{border:1px solid var(--line);background:#fafbfc;border-radius:8px;padding:10px 16px;margin:12px 0}.box ul{margin:6px 0;padding-left:20px}
.note{color:var(--muted);font-size:12px}.fp{color:var(--muted);font-size:12px;margin-top:2px}
.toolbar{position:sticky;top:0;background:#fff;padding:8px 0;margin:-8px 0 12px;border-bottom:1px solid var(--line);display:flex;gap:10px;align-items:center}
button{font:inherit;padding:6px 14px;border:1px solid var(--ink);background:var(--ink);color:#fff;border-radius:6px;cursor:pointer}
.part{page-break-before:always}.part:first-of-type{page-break-before:auto}
.mono{font-family:Consolas,"D2Coding",monospace;font-size:11.5px}
.tl td:nth-child(1),.tl td:nth-child(2),.tl td:nth-child(3),.tl td:nth-child(6),.tl th{white-space:nowrap;word-break:keep-all}.tl td:nth-child(7){width:30%}
@media print{body{background:#fff}.page{padding:0;max-width:none}.toolbar{display:none}h2{page-break-after:avoid}tr{page-break-inside:avoid}}
"#;

/// 보고서 HTML 전체.
pub fn render(case: &CaseData, lang: Lang, regenerated: Option<DateTime<Utc>>) -> String {
    let tz: Tz = case.info.time_zone.parse().unwrap_or(chrono_tz::Asia::Seoul);
    let c = Ctx { lang, tz, mask: case.info.mask_names, case };
    let mut h = String::with_capacity(64 * 1024);
    let i = &case.info;
    let a = &case.analysis;
    let asmt = &case.assessment;

    let _ = write!(
        h,
        "<!DOCTYPE html><html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} — {}</title><style>{CSS}</style></head><body><div class=\"page\">",
        lang.code(),
        esc(&c.t("report.title")),
        esc(&i.pc_name)
    );
    let _ = write!(
        h,
        "<div class=\"toolbar\"><button onclick=\"window.print()\">{}</button><span class=\"note\">{}</span></div>",
        esc(&c.t("report.print")),
        esc(&c.t("report.print_hint"))
    );
    let _ = write!(h, "<h1>{}</h1><p class=\"sub\">{} · {}</p>", esc(&c.t("report.title")), esc(&c.t("app.name")), esc(&c.t("app.tagline")));

    // ===== 1부 =====
    let _ = write!(h, "<section class=\"part\"><h2>1. {}</h2>", esc(&c.t("report.part1")));
    let period = match (asmt.period_start, i.period_days) {
        (Some(s), d) if d > 0 => format!("{} ~ {} ({d}{})", c.when(Some(s)), c.when(asmt.period_end), if lang == Lang::Ko { "일" } else { " days" }),
        _ => c.t("report.period_all"),
    };
    let mut meta = vec![
        (c.t("report.customer"), i.customer.clone()),
        (c.t("report.pc"), i.pc_name.clone()),
        (c.t("report.analyst"), i.analyst.clone()),
        (c.t("report.company"), i.company.clone()),
        (c.t("report.period"), period),
        (c.t("report.mode"), c.t(if i.mode == "live" { "report.mode_live" } else { "report.mode_offline" })),
        (c.t("report.started"), format_stamp_with_zone(i.started_utc, tz, lang)),
        (c.t("report.created"), format_stamp_with_zone(i.finished_utc, tz, lang)),
    ];
    if let Some(r) = regenerated {
        meta.push((c.t("report.regenerated"), format_stamp_with_zone(r, tz, lang)));
    }
    h.push_str("<table class=\"meta\">");
    for (k, v) in &meta {
        let _ = write!(h, "<tr><td>{}</td><td>{}</td></tr>", esc(k), esc(if v.is_empty() { "-" } else { v }));
    }
    let _ = write!(h, "</table><p class=\"note\">{}</p>", esc(&c.t("report.clock_note")));

    let (cls, desc) = match asmt.grade {
        Grade::NothingFound => ("g-nothing", "grade.desc.nothing"),
        Grade::Caution => ("g-caution", "grade.desc.caution"),
        Grade::Review => ("g-review", "grade.desc.review"),
        Grade::Undetermined => ("g-undetermined", "grade.desc.undetermined"),
    };
    let _ = write!(
        h,
        "<div class=\"grade {cls}\"><div class=\"light\"></div><div><div class=\"label\">{}</div><div class=\"desc\">{}</div></div></div>",
        esc(&c.t(asmt.grade.key())),
        esc(&c.t(desc))
    );
    if !i.admin && i.mode == "live" {
        let _ = write!(h, "<div class=\"box\">{}</div>", esc(&c.t("ui.limited")));
    }

    let _ = write!(h, "<h3>{}</h3>", esc(&c.t("report.key_events")));
    let key: Vec<&Finding> = asmt.findings.iter().filter(|f| f.level >= Level::Medium).take(5).collect();
    if key.is_empty() {
        let _ = write!(h, "<p>{}</p>", esc(&c.t("report.no_key_events")));
    } else {
        h.push_str("<ol class=\"key\">");
        for f in key {
            let _ = write!(
                h,
                "<li><span class=\"lv lv-{:?}\">{}</span>{}<div class=\"fp\">{}</div></li>",
                f.level,
                esc(&c.t(&format!("level.{:?}", f.level))),
                esc(&finding_sentence(f, lang, tz)),
                esc(&c.t(&format!("finding.{}.fp", f.rule_id)))
            );
        }
        h.push_str("</ol>");
    }

    let _ = write!(h, "<h3>{}</h3><ul>", esc(&c.t("report.next_steps")));
    let mut steps = Vec::new();
    let has_devices = !a.devices.is_empty();
    match asmt.grade {
        Grade::Review => {
            steps.push("next.review");
            if has_devices {
                steps.push("next.device");
            }
            steps.push("next.keep");
        }
        Grade::Caution => {
            if has_devices {
                steps.push("next.device");
            }
            steps.push("next.keep");
        }
        Grade::Undetermined => steps.extend(["next.rerun_admin", "next.keep"]),
        Grade::NothingFound => steps.extend(["next.nothing"]),
    }
    for s in steps {
        let _ = write!(h, "<li>{}</li>", esc(&c.t(s)));
    }
    h.push_str("</ul>");

    let _ = write!(h, "<div class=\"box\"><b>{}</b><ul>", esc(&c.t("report.cannot_check")));
    for k in ["cannot.1", "cannot.2", "cannot.3", "cannot.4", "cannot.5"] {
        let _ = write!(h, "<li>{}</li>", esc(&c.t(k)));
    }
    let missing: Vec<String> = case
        .checks
        .iter()
        .filter(|ch| ch.count == 0)
        .map(|ch| c.t(&format!("artifact.{}", ch.artifact)))
        .collect();
    if !missing.is_empty() {
        let _ = write!(h, "<li>{}: {}</li>", esc(&c.t("report.missing_data")), esc(&missing.join(", ")));
    }
    let _ = write!(h, "</ul></div><p class=\"note\">{}</p></section>", esc(&c.t("report.reference_only")));

    // ===== 2부 =====
    let _ = write!(h, "<section class=\"part\"><h2>2. {}</h2>", esc(&c.t("report.part2")));

    // 판단 근거
    let _ = write!(h, "<h3>{}</h3>", esc(&c.t("report.findings")));
    if asmt.findings.is_empty() {
        let _ = write!(h, "<p>{}</p>", esc(&c.t("report.none")));
    } else {
        let _ = write!(
            h,
            "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
            esc(&c.t("col.level")),
            esc(&c.t("col.rule")),
            esc(&c.t("col.description")),
            esc(&c.t("col.records"))
        );
        for f in &asmt.findings {
            let refs: Vec<String> = f.evidence.iter().take(20).map(|n| format!("#{}", n + 1)).collect();
            let more = if f.evidence.len() > 20 { format!(" … ({})", f.evidence.len()) } else { String::new() };
            let _ = write!(
                h,
                "<tr><td><span class=\"lv lv-{:?}\">{}</span></td><td>{} {}</td><td>{}<div class=\"fp\">{}</div></td><td class=\"mono\">{}{}</td></tr>",
                f.level,
                esc(&c.t(&format!("level.{:?}", f.level))),
                esc(&f.rule_id),
                esc(&c.t(&format!("finding.{}.title", f.rule_id))),
                esc(&finding_sentence(f, lang, tz)),
                esc(&c.t(&format!("finding.{}.fp", f.rule_id))),
                esc(&refs.join(" ")),
                esc(&more)
            );
        }
        h.push_str("</table>");
    }

    // 이동식 저장장치별
    if !asmt.incidents.is_empty() {
        let _ = write!(
            h,
            "<h3>{}</h3><table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
            esc(&c.t("report.incidents")),
            esc(&c.t("col.volume")),
            esc(&c.t("col.device")),
            esc(&c.t("col.link")),
            esc(&c.t("col.opened")),
            esc(&c.t("col.first")),
            esc(&c.t("col.last"))
        );
        for inc in &asmt.incidents {
            let _ = write!(
                h,
                "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(if inc.volume_serial.is_empty() { "-" } else { &inc.volume_serial }),
                esc(inc.device_label.as_deref().unwrap_or("-")),
                esc(&c.t(&format!("link.{:?}", inc.link))),
                inc.opened.len(),
                esc(&c.when(inc.first)),
                esc(&c.when(inc.last))
            );
        }
        h.push_str("</table>");
    }

    // 실행한 프로그램
    render_programs(&mut h, &c);
    // 원격 접속
    render_remote(&mut h, &c);
    // 전송량
    render_transfer(&mut h, &c);
    // 이동식·네트워크·휴대 장치의 폴더
    render_folders(&mut h, &c);

    // 시간순 기록 (최근 것부터 TIMELINE_LIMIT건, 번호는 근거 번호와 같다)
    let _ = write!(
        h,
        "<h3>{}</h3><p class=\"note\">{}</p><table class=\"tl\"><tr><th>#</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.timeline")),
        esc(&tf(lang, "report.timeline_note", &[("limit", &TIMELINE_LIMIT.to_string())])),
        esc(&c.t("col.time")),
        esc(&c.t("col.kind")),
        esc(&c.t("col.subject")),
        esc(&c.t("col.account")),
        esc(&c.t("col.confidence")),
        esc(&c.t("col.source"))
    );
    let tool: std::collections::BTreeSet<usize> = asmt.tool_media_events.iter().copied().collect();
    let mut shown: Vec<(usize, &Event)> = a.events.iter().enumerate().collect();
    // 근거로 쓰인 기록은 항상 싣는다.
    let cited: std::collections::BTreeSet<usize> = asmt.findings.iter().flat_map(|f| f.evidence.iter().copied()).collect();
    shown.sort_by_key(|x| std::cmp::Reverse(x.1.time.utc));
    let mut picked: Vec<(usize, &Event)> = shown.iter().filter(|(n, _)| cited.contains(n)).copied().collect();
    for item in shown.iter() {
        if picked.len() >= TIMELINE_LIMIT.max(cited.len()) {
            break;
        }
        if !cited.contains(&item.0) {
            picked.push(*item);
        }
    }
    picked.sort_by(|x, y| y.1.time.utc.cmp(&x.1.time.utc).then(x.0.cmp(&y.0)));
    for (n, e) in picked {
        let kind = format!("{:?}", e.kind);
        let mut subject = c.subject(&e.subject);
        if tool.contains(&n) {
            subject = format!("{subject} [{}]", c.t("report.tool_media"));
        }
        let extra = event_extra(e, &c);
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}{}</td><td>{}</td><td>{}</td><td class=\"mono\">{}</td></tr>",
            n + 1,
            esc(&c.when(e.time.utc)),
            esc(&c.t(&format!("kind.{kind}"))),
            esc(&subject),
            if extra.is_empty() { String::new() } else { format!("<div class=\"note\">{}</div>", esc(&extra)) },
            esc(&c.account(e.account.name.as_deref().or(e.account.sid.as_deref()).unwrap_or(""))),
            esc(&c.t(&format!("confidence.{:?}", e.confidence))),
            esc(&format!("{} {}", e.source.file, e.source.record))
        );
    }
    h.push_str("</table>");

    // 자료별 범위
    let _ = write!(
        h,
        "<h3>{}</h3><table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.coverage")),
        esc(&c.t("col.data")),
        esc(&c.t("col.files")),
        esc(&c.t("col.records")),
        esc(&c.t("col.range"))
    );
    for cov in &a.coverage {
        let range = if cov.files == 0 {
            c.t("coverage.not_found")
        } else if cov.earliest.is_none() {
            c.t("coverage.no_records")
        } else {
            format!("{} ~ {}", c.when(cov.earliest), c.when(cov.latest))
        };
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&c.t(&format!("artifact.{}", cov.artifact))),
            cov.files,
            cov.events,
            esc(&range)
        );
    }
    h.push_str("</table>");

    // 장치
    let _ = write!(
        h,
        "<h3>{}</h3>",
        esc(&c.t("report.devices"))
    );
    if a.devices.is_empty() {
        let _ = write!(h, "<p>{}</p>", esc(&c.t("report.none")));
    } else {
        let _ = write!(
            h,
            "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
            esc(&c.t("col.kind")),
            esc(&c.t("col.device")),
            esc(&c.t("col.serial")),
            esc(&c.t("col.first")),
            esc(&c.t("col.last")),
            esc(&c.t("col.removed")),
            esc(&c.t("col.letters"))
        );
        for d in &a.devices {
            let name = [d.vendor.clone(), d.product.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ");
            let name = if name.is_empty() { d.friendly_name.clone().unwrap_or_default() } else { name };
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}</td><td class=\"mono\">{}{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&c.t(&format!("device_kind.{:?}", d.kind))),
                esc(&name),
                esc(d.serial.as_deref().unwrap_or("-")),
                if d.serial_is_unique { "" } else { " *" },
                esc(&c.when(d.first_install)),
                esc(&c.when(d.last_arrival)),
                esc(&c.when(d.last_removal)),
                esc(&d.drive_letters.join(", "))
            );
        }
        h.push_str("</table>");
    }

    // 네트워크
    let _ = write!(h, "<h3>{}</h3>", esc(&c.t("report.networks")));
    if a.networks.is_empty() {
        let _ = write!(h, "<p>{}</p>", esc(&c.t("report.none")));
    } else {
        let _ = write!(
            h,
            "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
            esc(&c.t("col.name")),
            esc(&c.t("col.medium")),
            esc(&c.t("col.first")),
            esc(&c.t("col.last"))
        );
        for n in &a.networks {
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&n.name),
                esc(&c.t(&format!("medium.{}", n.medium))),
                esc(&c.when(n.created_utc)),
                esc(&c.when(n.last_connected_utc))
            );
        }
        h.push_str("</table>");
    }

    // PC 정보와 계정
    let s = &a.system;
    let _ = write!(h, "<h3>{}</h3><table class=\"meta\">", esc(&c.t("report.system")));
    let os = [s.product_name.clone(), s.display_version.clone(), s.build.clone().map(|b| format!("build {b}"))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    for (k, v) in [
        ("sys.computer", s.computer_name.clone().unwrap_or_default()),
        ("sys.os", os),
        ("sys.install", c.when(s.install_date)),
        ("sys.shutdown", c.when(s.last_shutdown)),
        ("sys.tz", s.time_zone_key.clone().unwrap_or_default()),
    ] {
        let _ = write!(h, "<tr><td>{}</td><td>{}</td></tr>", esc(&c.t(k)), esc(if v.is_empty() { "-" } else { &v }));
    }
    h.push_str("</table>");
    if !a.accounts.is_empty() {
        let _ = write!(
            h,
            "<h3>{}</h3><table><tr><th>{}</th><th>SID</th><th>{}</th></tr>",
            esc(&c.t("report.accounts")),
            esc(&c.t("col.name")),
            esc(&c.t("col.profile"))
        );
        for acc in &a.accounts {
            let _ = write!(
                h,
                "<tr><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
                esc(&c.account(&acc.name)),
                esc(&acc.sid),
                esc(&c.subject(&acc.profile_path))
            );
        }
        h.push_str("</table>");
    }

    // 변조 확인
    let _ = write!(h, "<h3>{}</h3><table class=\"meta\">", esc(&c.t("report.integrity")));
    let _ = write!(h, "<tr><td>{}</td><td>{}</td></tr>", esc(&c.t("report.tool_version")), esc(&i.tool_version));
    let _ = write!(
        h,
        "<tr><td>{}</td><td>{}</td></tr>",
        esc(&c.t("report.input_unchanged")),
        esc(&c.t(if i.input_unchanged { "report.no" } else { "report.yes" }))
    );
    let _ = write!(
        h,
        "<tr><td>{}</td><td>{}</td></tr>",
        esc(&c.t("report.consent")),
        esc(&c.t(if i.consent { "report.yes" } else { "report.no" }))
    );
    if let Some(v) = &i.tool_media.volume_serial {
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{} {}</td></tr>",
            esc(&c.t("report.tool_media")),
            esc(i.tool_media.drive_letter.as_deref().unwrap_or("")),
            esc(v)
        );
    }
    h.push_str("</table>");
    if !case.collection.is_empty() {
        let _ = write!(h, "<table><tr><th>{}</th><th>{}</th><th>{}</th></tr>", esc(&c.t("col.data")), esc(&c.t("col.description")), esc(&c.t("col.hash")));
        for f in &case.collection {
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}{}</td><td class=\"mono\">{}</td></tr>",
                esc(&c.t(&format!("artifact.{}", f.artifact))),
                esc(&f.method),
                if f.ok { String::new() } else { format!(" — {}", esc(&f.message)) },
                esc(&f.sha256)
            );
        }
        h.push_str("</table>");
    }
    let _ = write!(h, "<table><tr><th>{}</th><th>{}</th></tr>", esc(&c.t("col.data")), esc(&c.t("col.hash")));
    for (k, v) in &case.input_hashes {
        let _ = write!(h, "<tr><td class=\"mono\">{}</td><td class=\"mono\">{}</td></tr>", esc(&c.subject(k)), esc(v));
    }
    h.push_str("</table>");

    // 읽지 못한 항목
    if !a.issues.is_empty() {
        let _ = write!(
            h,
            "<h3>{}</h3><table><tr><th>{}</th><th>{}</th><th>{}</th></tr>",
            esc(&c.t("report.issues")),
            esc(&c.t("col.data")),
            esc(&c.t("col.source")),
            esc(&c.t("col.note"))
        );
        for is in a.issues.iter().take(200) {
            let _ = write!(
                h,
                "<tr><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
                esc(&c.t(&format!("artifact.{}", is.artifact))),
                esc(&c.subject(&is.file)),
                esc(&is.reason)
            );
        }
        h.push_str("</table>");
    }

    // 용어·한계
    let _ = write!(h, "<h3>{}</h3><ul>", esc(&c.t("report.glossary")));
    for k in ["gloss.1", "gloss.2", "gloss.3", "gloss.4", "gloss.5", "gloss.6", "gloss.7", "gloss.8"] {
        let _ = write!(h, "<li>{}</li>", esc(&c.t(k)));
    }
    let _ = write!(h, "</ul><h3>{}</h3><ul>", esc(&c.t("report.limits")));
    for k in ["limit.1", "limit.2", "limit.3"] {
        let _ = write!(h, "<li>{}</li>", esc(&c.t(k)));
    }
    let _ = write!(h, "</ul><p class=\"note\">{}</p></section></div></body></html>", esc(&c.t("report.reference_only")));
    h
}

const SECTION_LIMIT: usize = 100;

fn base_of(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn render_programs(h: &mut String, c: &Ctx) {
    use st_core::EventKind;
    let a = &c.case.analysis;
    let flagged: std::collections::BTreeSet<usize> = c
        .case
        .assessment
        .findings
        .iter()
        .filter(|f| f.rule_id == "R9")
        .flat_map(|f| f.evidence.iter().copied())
        .collect();
    struct Row {
        path: String,
        last: Option<DateTime<Utc>>,
        runs: u64,
        sources: std::collections::BTreeSet<String>,
        removable: bool,
    }
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for (i, e) in a.events.iter().enumerate() {
        if e.kind != EventKind::ProgramRun {
            continue;
        }
        let key = e.subject.to_lowercase();
        let r = rows.entry(key).or_insert_with(|| Row {
            path: e.subject.clone(),
            last: None,
            runs: 0,
            sources: Default::default(),
            removable: false,
        });
        if e.time.utc > r.last {
            r.last = e.time.utc;
        }
        if let Some(n) = e.details.get("run_count").and_then(|v| v.parse::<u64>().ok()) {
            r.runs = r.runs.max(n);
        }
        r.sources.insert(c.t(&format!("artifact.{}", e.source.artifact)).split('(').nth(1).map(|x| x.trim_end_matches(')').to_string()).unwrap_or_else(|| e.source.artifact.clone()));
        if flagged.contains(&i) {
            r.removable = true;
        }
    }
    if rows.is_empty() {
        return;
    }
    let mut list: Vec<Row> = rows.into_values().collect();
    list.sort_by(|x, y| y.removable.cmp(&x.removable).then(y.last.cmp(&x.last)));
    let _ = write!(
        h,
        "<h3>{}</h3><p class=\"note\">{}</p><table class=\"tl\"><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.programs")),
        esc(&tf(c.lang, "report.programs_note", &[("limit", &SECTION_LIMIT.to_string())])),
        esc(&c.t("col.last")),
        esc(&c.t("col.program")),
        esc(&c.t("col.runs")),
        esc(&c.t("col.evidence"))
    );
    for r in list.iter().take(SECTION_LIMIT) {
        let flag = if r.removable { format!(" <span class=\"lv lv-Medium\">{}</span>", esc(&c.t("col.from_removable"))) } else { String::new() };
        let _ = write!(
            h,
            "<tr><td>{}</td><td><b>{}</b>{flag}<div class=\"note\">{}</div></td><td>{}</td><td>{}</td></tr>",
            esc(&c.when(r.last)),
            esc(base_of(&r.path)),
            esc(&c.subject(&r.path)),
            if r.runs > 0 { r.runs.to_string() } else { "-".into() },
            esc(&r.sources.iter().cloned().collect::<Vec<_>>().join(", "))
        );
    }
    h.push_str("</table>");
}

fn render_remote(h: &mut String, c: &Ctx) {
    use st_core::EventKind;
    let mut ev: Vec<&Event> = c.case.analysis.events.iter().filter(|e| e.kind == EventKind::RemoteLogon).collect();
    if ev.is_empty() {
        return;
    }
    ev.sort_by(|x, y| y.time.utc.cmp(&x.time.utc));
    let _ = write!(
        h,
        "<h3>{}</h3><table class=\"tl\"><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.remote")),
        esc(&c.t("col.time")),
        esc(&c.t("col.action")),
        esc(&c.t("col.ip")),
        esc(&c.t("col.account"))
    );
    for e in ev.iter().take(SECTION_LIMIT) {
        let action = e.details.get("action").map(|a| c.t(&format!("action.{a}"))).unwrap_or_default();
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
            esc(&c.when(e.time.utc)),
            esc(&action),
            esc(e.details.get("source_ip").map(String::as_str).unwrap_or("-")),
            esc(&c.account(e.account.name.as_deref().or(e.account.sid.as_deref()).unwrap_or("")))
        );
    }
    h.push_str("</table>");
}

fn render_transfer(h: &mut String, c: &Ctx) {
    use st_core::EventKind;
    let mut sums: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for e in c.case.analysis.events.iter().filter(|e| e.kind == EventKind::NetworkUsage) {
        let s = e.details.get("bytes_sent").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let r = e.details.get("bytes_received").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let x = sums.entry(e.subject.clone()).or_insert((0, 0));
        x.0 = x.0.saturating_add(s);
        x.1 = x.1.saturating_add(r);
    }
    if sums.is_empty() {
        return;
    }
    let mut list: Vec<(String, (u64, u64))> = sums.into_iter().collect();
    list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    const TOP: usize = 15;
    let max = list.first().map(|x| x.1 .0).unwrap_or(1).max(1);
    let _ = write!(
        h,
        "<h3>{}</h3><p class=\"note\">{}</p><table><tr><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.transfer")),
        esc(&tf(c.lang, "report.transfer_note", &[("limit", &TOP.to_string())])),
        esc(&c.t("col.program")),
        esc(&c.t("col.sent")),
        esc(&c.t("col.received"))
    );
    for (app, (sent, recv)) in list.iter().take(TOP) {
        let pct = (*sent as f64 / max as f64 * 100.0).round();
        let _ = write!(
            h,
            "<tr><td><b>{}</b><div class=\"note\">{}</div></td><td style=\"width:34%\"><div class=\"bar\"><span style=\"width:{pct}%\"></span></div>{}</td><td>{}</td></tr>",
            esc(base_of(app)),
            esc(&c.subject(app)),
            esc(&bytes_text(*sent)),
            esc(&bytes_text(*recv))
        );
    }
    h.push_str("</table>");
}

fn bytes_text(b: u64) -> String {
    let mb = b as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 1.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{} KB", b / 1024)
    }
}

fn render_folders(h: &mut String, c: &Ctx) {
    use st_core::EventKind;
    let interesting = |e: &&Event| -> bool {
        if e.kind != EventKind::FolderOpened {
            return false;
        }
        let s = e.subject.as_str();
        let letter = s.get(..2).map(str::to_ascii_uppercase).unwrap_or_default();
        s.starts_with("\\\\")
            || (letter.ends_with(':') && letter != "C:")
            || e.details.get("root").is_some_and(|r| r == "Other" || r == "Network")
    };
    let mut ev: Vec<&Event> = c.case.analysis.events.iter().filter(interesting).collect();
    if ev.is_empty() {
        return;
    }
    ev.sort_by(|x, y| y.time.utc.cmp(&x.time.utc).then(x.subject.cmp(&y.subject)));
    let _ = write!(
        h,
        "<h3>{}</h3><table class=\"tl\"><tr><th>{}</th><th>{}</th><th>{}</th></tr>",
        esc(&c.t("report.folders")),
        esc(&c.t("col.time")),
        esc(&c.t("col.folder")),
        esc(&c.t("col.account"))
    );
    for e in ev.iter().take(SECTION_LIMIT) {
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&c.when(e.time.utc)),
            esc(&c.subject(&e.subject)),
            esc(&c.account(e.account.name.as_deref().unwrap_or("")))
        );
    }
    h.push_str("</table>");
}

/// 시간순 기록 칸에 덧붙일 짧은 정보.
fn event_extra(e: &Event, c: &Ctx) -> String {
    let d: &BTreeMap<String, String> = &e.details;
    let mut parts = Vec::new();
    for k in [
        "drive_type",
        "volume_serial",
        "device_serial",
        "medium",
        "browser",
        "app",
        "run_count",
        "source_ip",
        "action",
        "previous_time",
        "new_time",
        "by_user",
        "size_bytes",
        "sha1",
    ] {
        if let Some(v) = d.get(k).filter(|v| !v.is_empty()) {
            let v = if k.ends_with("_time") { c.when_str(v) } else { v.clone() };
            let v = if k == "by_user" { c.account(&v) } else { v };
            parts.push(format!("{k}={v}"));
        }
    }
    let _ = c.case;
    parts.join(" · ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn escapes_everything() {
        assert_eq!(esc("<script>alert('x')</script>&\""), "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;&quot;");
    }

    #[test]
    fn masks_user_folder_and_file_name() {
        assert_eq!(mask_user_dir("C:\\Users\\kim\\Desktop"), "C:\\Users\\***\\Desktop");
    }
}
