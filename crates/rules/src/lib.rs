//! st-rules: 사실(Event)을 연결하고(상관 분석) 판단 규칙을 적용한다.
//!
//! 원칙 (개발 요청서 2장, 7장)
//! - 판단은 사실과 분리해 `Finding`으로 남긴다. 각 판단은 근거 사실의 위치(번호)를 가진다.
//! - 단정하지 않는다. 등급은 "특이사항 미발견(분석 범위 내) / 주의 / 확인 필요 / 판단 불가".
//! - 점검 도구가 담긴 매체의 기록은 판단에서 뺀다.
//! - 회사 장치 목록에 있는 장치는 판단에서 뺀다.
//!
//! 규칙과 기준값은 `st_core::settings::RuleConfig`로 켜고 끄거나 바꿀 수 있다.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use st_core::settings::{Customer, RuleConfig};
use st_core::{DriveInfo, Event, EventKind};
use st_timeline::Analysis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    /// 특이사항 미발견 (분석 범위 내)
    NothingFound,
    /// 주의
    Caution,
    /// 확인 필요
    Review,
    /// 판단 불가 (필수 자료 부족)
    Undetermined,
}

impl Grade {
    pub fn key(self) -> &'static str {
        match self {
            Grade::NothingFound => "grade.nothing",
            Grade::Caution => "grade.caution",
            Grade::Review => "grade.review",
            Grade::Undetermined => "grade.undetermined",
        }
    }
}

/// 규칙 하나가 내린 판단.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub level: Level,
    /// 문장 틀에 채울 값 (예: device, count, time)
    pub params: BTreeMap<String, String>,
    /// 근거 사실 번호 (Analysis.events의 위치)
    pub evidence: Vec<usize>,
    /// 대표 시각 (정렬용)
    pub time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkStrength {
    /// 볼륨 시리얼 번호가 일치
    Direct,
    /// 드라이브 문자와 시각만 맞음
    Circumstantial,
    /// 장치를 특정하지 못함
    Unlinked,
}

/// 이동식 볼륨 하나를 중심으로 묶은 사건.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub volume_serial: String,
    pub device_label: Option<String>,
    pub device_serial: Option<String>,
    pub link: LinkStrength,
    pub opened: Vec<usize>,
    pub attached: Vec<usize>,
    pub first: Option<DateTime<Utc>>,
    pub last: Option<DateTime<Utc>>,
    pub allowed: bool,
}

/// 점검 도구가 들어 있는 매체.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolMedia {
    pub drive_letter: Option<String>,
    pub volume_serial: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub grade: Grade,
    pub findings: Vec<Finding>,
    pub incidents: Vec<Incident>,
    /// 점검 도구 매체 때문에 생긴 것으로 보고 판단에서 뺀 사실 번호
    pub tool_media_events: Vec<usize>,
    /// 필수 자료가 없어 판단 범위가 줄었다면 그 이유
    pub missing_required: Vec<String>,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
}

pub struct Context<'a> {
    pub rules: &'a RuleConfig,
    pub customer: &'a Customer,
    pub tz: Tz,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub tool_media: &'a ToolMedia,
    /// 점검 시점의 드라이브 (이동식 드라이브 문자 판단용). 모르면 빈 목록.
    pub drives: &'a [DriveInfo],
}

impl Context<'_> {
    fn in_period(&self, t: Option<DateTime<Utc>>) -> bool {
        let Some(t) = t else { return false };
        self.period_start.is_none_or(|s| t >= s) && self.period_end.is_none_or(|e| t <= e)
    }

    /// 업무 시간 밖인가 (표시 시간대 기준)
    fn after_hours(&self, t: DateTime<Utc>) -> bool {
        let local = t.with_timezone(&self.tz);
        let day_bit = 1u8 << local.weekday().num_days_from_monday();
        let workday = self.customer.work_days_mask & day_bit != 0;
        let h = local.hour();
        !workday || h < self.customer.work_start_hour || h >= self.customer.work_end_hour
    }
}

fn detail<'a>(e: &'a Event, k: &str) -> Option<&'a str> {
    e.details.get(k).map(String::as_str)
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    Some(host.to_ascii_lowercase())
}

fn domain_match(host: &str, list: &[String]) -> bool {
    list.iter().any(|d| {
        let d = d.trim().to_ascii_lowercase();
        !d.is_empty() && (host == d || host.ends_with(&format!(".{d}")))
    })
}

/// 경로의 마지막 부분(파일 이름)을 소문자로.
fn base_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// 사설·예약 주소인가 (회사 안 접속으로 본다).
fn private_ip(ip: &str) -> bool {
    let ip = ip.trim();
    if ip.is_empty() || ip == "-" || ip == "127.0.0.1" || ip == "::1" || ip.eq_ignore_ascii_case("LOCAL") {
        return true;
    }
    if let Ok(v4) = ip.parse::<std::net::Ipv4Addr>() {
        return v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified();
    }
    if let Ok(v6) = ip.parse::<std::net::Ipv6Addr>() {
        let first = v6.segments().first().copied().unwrap_or(0);
        return v6.is_loopback() || v6.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80;
    }
    false
}

fn human_bytes(b: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = b as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

fn fmt_time(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.to_rfc3339()).unwrap_or_default()
}

/// 상관 분석과 판단을 한 번에 한다.
pub fn assess(a: &Analysis, ctx: &Context) -> Assessment {
    let ev = &a.events;
    let r = ctx.rules;
    let mut findings = Vec::new();

    // --- 점검 도구 매체 ---
    let tool_vsn = ctx.tool_media.volume_serial.as_deref().map(str::to_ascii_uppercase);
    let tool_letter = ctx.tool_media.drive_letter.as_deref().map(|l| l.trim_end_matches('\\').to_ascii_uppercase());
    let tool_device_serials: BTreeSet<String> = a
        .devices
        .iter()
        .filter(|d| {
            tool_letter.as_ref().is_some_and(|l| d.drive_letters.iter().any(|x| x.eq_ignore_ascii_case(l)))
        })
        .filter_map(|d| d.serial.clone())
        .collect();
    let is_tool = |e: &Event| -> bool {
        let vsn = detail(e, "volume_serial").map(str::to_ascii_uppercase);
        if tool_vsn.is_some() && vsn == tool_vsn {
            return true;
        }
        detail(e, "device_serial").is_some_and(|s| tool_device_serials.contains(s))
    };
    let tool_media_events: Vec<usize> = ev.iter().enumerate().filter(|(_, e)| is_tool(e)).map(|(i, _)| i).collect();
    let tool_set: BTreeSet<usize> = tool_media_events.iter().copied().collect();

    let allowed = |e: &Event| -> bool {
        detail(e, "device_serial").is_some_and(|s| ctx.customer.is_allowed(s))
            || detail(e, "volume_serial").is_some_and(|s| ctx.customer.is_allowed(s))
    };

    // --- 상관 분석: 이동식 볼륨별 사건 ---
    // 볼륨 시리얼 → (장치 일련번호, 장치 이름): 저장장치 연결 이벤트(1006)에서 얻는다.
    let mut vsn_to_device: BTreeMap<String, (Option<String>, String)> = BTreeMap::new();
    let mut attached_by_vsn: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in ev.iter().enumerate() {
        if e.kind == EventKind::StorageAttached && detail(e, "removable") != Some("false") {
            if let Some(v) = detail(e, "volume_serial") {
                let v = v.to_ascii_uppercase();
                vsn_to_device.insert(v.clone(), (detail(e, "device_serial").map(str::to_string), e.subject.clone()));
                attached_by_vsn.entry(v).or_default().push(i);
            }
        }
    }
    let mut opened_by_vsn: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in ev.iter().enumerate() {
        if e.kind == EventKind::FileOpened && detail(e, "drive_type") == Some("removable") && !tool_set.contains(&i) {
            let v = detail(e, "volume_serial").unwrap_or("").to_ascii_uppercase();
            opened_by_vsn.entry(v).or_default().push(i);
        }
    }
    let mut incidents = Vec::new();
    for (vsn, opened) in &opened_by_vsn {
        let dev = vsn_to_device.get(vsn);
        // 드라이브 문자로 정황 연결: 바로 가기 경로의 드라이브 문자가 어떤 저장장치에 할당된 적이 있는가
        let letter_dev = opened.first().and_then(|&i| ev.get(i)).and_then(|e| {
            let l = e.subject.get(..2)?.to_ascii_uppercase();
            a.devices.iter().find(|d| d.drive_letters.iter().any(|x| x.eq_ignore_ascii_case(&l)))
        });
        let link = if vsn.is_empty() {
            LinkStrength::Unlinked
        } else if dev.is_some() {
            LinkStrength::Direct
        } else if letter_dev.is_some() {
            LinkStrength::Circumstantial
        } else {
            LinkStrength::Unlinked
        };
        let device_serial = dev.and_then(|d| d.0.clone()).or_else(|| letter_dev.and_then(|d| d.serial.clone()));
        let device_label = dev.map(|d| d.1.clone()).or_else(|| {
            letter_dev.map(|d| {
                [d.vendor.clone(), d.product.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ")
            })
        });
        let times: Vec<DateTime<Utc>> = opened.iter().filter_map(|&i| ev.get(i)?.time.utc).collect();
        let is_allowed = ctx.customer.is_allowed(vsn)
            || device_serial.as_deref().is_some_and(|s| ctx.customer.is_allowed(s));
        incidents.push(Incident {
            volume_serial: vsn.clone(),
            device_label,
            device_serial,
            link,
            opened: opened.clone(),
            attached: attached_by_vsn.get(vsn).cloned().unwrap_or_default(),
            first: times.iter().min().copied(),
            last: times.iter().max().copied(),
            allowed: is_allowed,
        });
    }

    // --- 규칙 ---
    // R1 처음 보는 저장장치
    if r.new_storage_device {
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::DeviceFirstConnected
                && detail(e, "device_kind") == Some("UsbStorage")
                && ctx.in_period(e.time.utc)
                && !tool_set.contains(&i)
                && !allowed(e)
            {
                let mut p = BTreeMap::new();
                p.insert("device".to_string(), e.subject.clone());
                p.insert("time".to_string(), fmt_time(e.time.utc));
                p.insert("serial".to_string(), detail(e, "device_serial").unwrap_or("").to_string());
                findings.push(Finding { rule_id: "R1".into(), level: Level::Medium, params: p, evidence: vec![i], time: e.time.utc });
            }
        }
    }
    // R2 업무 시간 외 저장장치 사용
    if r.after_hours_storage_use {
        for (i, e) in ev.iter().enumerate() {
            let storage_use = matches!(e.kind, EventKind::DeviceLastConnected | EventKind::DeviceFirstConnected)
                && detail(e, "device_kind") == Some("UsbStorage")
                || e.kind == EventKind::StorageAttached
                    && detail(e, "connected") == Some("true")
                    && detail(e, "removable") != Some("false");
            if storage_use
                && ctx.in_period(e.time.utc)
                && !tool_set.contains(&i)
                && !allowed(e)
                && e.time.utc.is_some_and(|t| ctx.after_hours(t))
            {
                let mut p = BTreeMap::new();
                p.insert("device".to_string(), e.subject.clone());
                p.insert("time".to_string(), fmt_time(e.time.utc));
                findings.push(Finding { rule_id: "R2".into(), level: Level::Medium, params: p, evidence: vec![i], time: e.time.utc });
            }
        }
    }
    // R3 이동식 저장장치의 파일 열람
    if r.files_opened_from_removable {
        for inc in &incidents {
            let in_period: Vec<usize> =
                inc.opened.iter().copied().filter(|&i| ctx.in_period(ev.get(i).and_then(|e| e.time.utc))).collect();
            if in_period.is_empty() || inc.allowed {
                continue;
            }
            let after_hours = in_period.iter().any(|&i| ev.get(i).and_then(|e| e.time.utc).is_some_and(|t| ctx.after_hours(t)));
            let level = match (inc.link, after_hours) {
                (LinkStrength::Direct, _) | (_, true) => Level::High,
                _ => Level::Medium,
            };
            let mut p = BTreeMap::new();
            p.insert("count".to_string(), in_period.len().to_string());
            p.insert("device".to_string(), inc.device_label.clone().unwrap_or_default());
            p.insert("volume_serial".to_string(), inc.volume_serial.clone());
            p.insert("link".to_string(), format!("{:?}", inc.link));
            p.insert("after_hours".to_string(), after_hours.to_string());
            p.insert("time".to_string(), fmt_time(inc.first));
            let mut evidence = in_period;
            evidence.extend(inc.attached.iter().copied());
            findings.push(Finding { rule_id: "R3".into(), level, params: p, evidence, time: inc.first });
        }
    }
    // R4 대량 삭제
    if r.mass_delete && r.mass_delete_count > 0 {
        let deletes: Vec<(usize, DateTime<Utc>)> = ev
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == EventKind::FileDeleted && ctx.in_period(e.time.utc))
            .filter_map(|(i, e)| Some((i, e.time.utc?)))
            .collect();
        let window = Duration::minutes(i64::from(r.mass_delete_window_minutes));
        let need = usize::try_from(r.mass_delete_count).unwrap_or(usize::MAX);
        let mut start = 0usize;
        let mut used_until: Option<DateTime<Utc>> = None;
        for end in 0..deletes.len() {
            let Some(&(_, t_end)) = deletes.get(end) else { break };
            while deletes.get(start).is_some_and(|&(_, t)| t_end - t > window) {
                start += 1;
            }
            let count = end + 1 - start;
            let fresh = used_until.is_none_or(|u| deletes.get(start).is_some_and(|&(_, t)| t > u));
            if count >= need && fresh {
                let ids: Vec<usize> = deletes.get(start..=end).unwrap_or(&[]).iter().map(|&(i, _)| i).collect();
                // 이 창이 끝날 때까지 이어지는 삭제를 모두 포함한다.
                let mut last = end;
                while deletes.get(last + 1).is_some_and(|&(_, t)| t - deletes.get(start).map_or(t, |s| s.1) <= window) {
                    last += 1;
                }
                let ids_all: Vec<usize> = deletes.get(start..=last).unwrap_or(&[]).iter().map(|&(i, _)| i).collect();
                let first_t = deletes.get(start).map(|d| d.1);
                used_until = deletes.get(last).map(|d| d.1);
                let mut p = BTreeMap::new();
                p.insert("count".to_string(), ids_all.len().max(ids.len()).to_string());
                p.insert("minutes".to_string(), r.mass_delete_window_minutes.to_string());
                p.insert("time".to_string(), fmt_time(first_t));
                findings.push(Finding { rule_id: "R4".into(), level: Level::Medium, params: p, evidence: ids_all, time: first_t });
            }
        }
    }
    // R5 로그 삭제
    if r.log_cleared {
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::LogCleared && ctx.in_period(e.time.utc) {
                let mut p = BTreeMap::new();
                p.insert("log".to_string(), e.subject.clone());
                p.insert("time".to_string(), fmt_time(e.time.utc));
                p.insert("account".to_string(), e.account.name.clone().or_else(|| detail(e, "by_user").map(str::to_string)).unwrap_or_default());
                findings.push(Finding { rule_id: "R5".into(), level: Level::High, params: p, evidence: vec![i], time: e.time.utc });
            }
        }
    }
    // R6 사람 계정의 큰 시각 변경 (Windows 시간 동기화 등 시스템 계정의 변경은 제외)
    if r.time_changed {
        let limit = i64::from(r.time_change_minutes) * 60;
        for (i, e) in ev.iter().enumerate() {
            if e.kind != EventKind::TimeChanged || !ctx.in_period(e.time.utc) {
                continue;
            }
            let by_person = e.account.sid.as_deref().is_some_and(|s| s.starts_with("S-1-5-21-"));
            let delta = detail(e, "delta_seconds").and_then(|d| d.parse::<i64>().ok()).unwrap_or(0);
            if by_person && delta.abs() >= limit {
                let mut p = BTreeMap::new();
                p.insert("minutes".to_string(), (delta / 60).to_string());
                p.insert("time".to_string(), fmt_time(e.time.utc));
                p.insert("account".to_string(), e.account.name.clone().or_else(|| detail(e, "by_user").map(str::to_string)).unwrap_or_default());
                findings.push(Finding { rule_id: "R6".into(), level: Level::High, params: p, evidence: vec![i], time: e.time.utc });
            }
        }
    }
    // R7 스마트폰 등 휴대 장치
    if r.portable_device {
        for (i, e) in ev.iter().enumerate() {
            if matches!(e.kind, EventKind::DeviceFirstConnected | EventKind::DeviceLastConnected)
                && detail(e, "device_kind") == Some("Portable")
                && ctx.in_period(e.time.utc)
                && !allowed(e)
            {
                let mut p = BTreeMap::new();
                p.insert("device".to_string(), e.subject.clone());
                p.insert("time".to_string(), fmt_time(e.time.utc));
                findings.push(Finding { rule_id: "R7".into(), level: Level::Low, params: p, evidence: vec![i], time: e.time.utc });
            }
        }
    }
    // R8 클라우드 저장소·웹메일·파일 공유 접속
    if r.cloud_or_webmail {
        let mut hits: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ev.iter().enumerate() {
            if e.kind != EventKind::WebVisit || !ctx.in_period(e.time.utc) {
                continue;
            }
            let Some(host) = host_of(&e.subject) else { continue };
            if domain_match(&host, &r.cloud_domains)
                || domain_match(&host, &r.webmail_domains)
                || domain_match(&host, &r.file_share_domains)
            {
                hits.entry(host).or_default().push(i);
            }
        }
        let total: usize = hits.values().map(Vec::len).sum();
        if total > 0 {
            let level = if total >= usize::try_from(r.cloud_visit_count).unwrap_or(usize::MAX) { Level::Medium } else { Level::Low };
            let mut p = BTreeMap::new();
            p.insert("count".to_string(), total.to_string());
            p.insert("sites".to_string(), hits.keys().cloned().collect::<Vec<_>>().join(", "));
            let evidence: Vec<usize> = hits.values().flatten().copied().collect();
            let time = evidence.iter().filter_map(|&i| ev.get(i)?.time.utc).min();
            p.insert("time".to_string(), fmt_time(time));
            findings.push(Finding { rule_id: "R8".into(), level, params: p, evidence, time });
        }
    }

    // --- 이동식 볼륨·드라이브 (R9용) ---
    let mut removable_vsn: BTreeSet<String> = BTreeSet::new();
    for e in ev {
        let is_removable = (e.kind == EventKind::StorageAttached && detail(e, "removable") != Some("false"))
            || (e.kind == EventKind::FileOpened && detail(e, "drive_type") == Some("removable"));
        if is_removable {
            if let Some(v) = detail(e, "volume_serial").filter(|v| !v.is_empty()) {
                removable_vsn.insert(v.to_ascii_uppercase());
            }
        }
    }
    let mut removable_letters: BTreeSet<String> = ctx
        .drives
        .iter()
        .filter(|d| d.removable)
        .map(|d| d.letter.to_ascii_uppercase())
        .collect();
    for d in &a.devices {
        if d.kind == st_timeline::DeviceKind::UsbStorage {
            for l in &d.drive_letters {
                removable_letters.insert(l.trim_end_matches('\\').to_ascii_uppercase());
            }
        }
    }
    let system_letter = ctx.drives.iter().find(|d| d.letter.eq_ignore_ascii_case("C:")).map(|d| d.letter.clone());
    let from_removable = |e: &Event| -> bool {
        if detail(e, "drive_type") == Some("removable") {
            return true;
        }
        if detail(e, "volume_serial").is_some_and(|v| removable_vsn.contains(&v.to_ascii_uppercase())) {
            return true;
        }
        let letter = e.subject.get(..2).map(str::to_ascii_uppercase).unwrap_or_default();
        letter.ends_with(':') && removable_letters.contains(&letter) && system_letter.as_deref() != Some(letter.as_str())
    };
    let is_tool_letter = |e: &Event| -> bool {
        tool_letter.as_ref().is_some_and(|l| e.subject.get(..2).is_some_and(|x| x.eq_ignore_ascii_case(l)))
    };

    // R9 이동식 저장장치에서 실행한 프로그램
    if r.program_from_removable {
        let mut by_prog: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::ProgramRun
                && ctx.in_period(e.time.utc)
                && !tool_set.contains(&i)
                && !is_tool_letter(e)
                && !allowed(e)
                && from_removable(e)
            {
                by_prog.entry(e.subject.clone()).or_default().push(i);
            }
        }
        for (prog, ids) in by_prog {
            let times: Vec<DateTime<Utc>> = ids.iter().filter_map(|&i| ev.get(i)?.time.utc).collect();
            let after_hours = times.iter().any(|t| ctx.after_hours(*t));
            let first = times.iter().min().copied();
            let mut p = BTreeMap::new();
            p.insert("program".to_string(), prog);
            p.insert("count".to_string(), ids.len().to_string());
            p.insert("time".to_string(), fmt_time(first));
            let level = if after_hours { Level::High } else { Level::Medium };
            findings.push(Finding { rule_id: "R9".into(), level, params: p, evidence: ids, time: first });
        }
    }

    // R10 흔적 지우기·완전 삭제 프로그램 실행
    if r.cleanup_tools {
        let names: BTreeSet<String> = r.cleanup_tool_names.iter().map(|n| n.trim().to_ascii_lowercase()).collect();
        let mut by_prog: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::ProgramRun && ctx.in_period(e.time.utc) && names.contains(&base_name(&e.subject)) {
                by_prog.entry(base_name(&e.subject)).or_default().push(i);
            }
        }
        for (prog, ids) in by_prog {
            let first = ids.iter().filter_map(|&i| ev.get(i)?.time.utc).min();
            let last = ids.iter().filter_map(|&i| ev.get(i)?.time.utc).max();
            let mut p = BTreeMap::new();
            p.insert("program".to_string(), prog);
            p.insert("count".to_string(), ids.len().to_string());
            p.insert("time".to_string(), fmt_time(last));
            findings.push(Finding { rule_id: "R10".into(), level: Level::High, params: p, evidence: ids, time: first });
        }
    }

    // R11 원격 데스크톱 접속
    if r.remote_logon {
        let mut by_ip: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::RemoteLogon
                && ctx.in_period(e.time.utc)
                && matches!(detail(e, "action"), Some("logon" | "auth" | "reconnect"))
            {
                let ip = detail(e, "source_ip").unwrap_or("").to_string();
                by_ip.entry(ip).or_default().push(i);
            }
        }
        for (ip, ids) in by_ip {
            let external = !private_ip(&ip);
            let times: Vec<DateTime<Utc>> = ids.iter().filter_map(|&i| ev.get(i)?.time.utc).collect();
            let after_hours = times.iter().any(|t| ctx.after_hours(*t));
            let level = match (external, after_hours) {
                (true, _) => Level::High,
                (false, true) => Level::Medium,
                _ => Level::Low,
            };
            let accounts: BTreeSet<String> =
                ids.iter().filter_map(|&i| ev.get(i)?.account.name.clone()).filter(|n| !n.is_empty()).collect();
            let mut p = BTreeMap::new();
            p.insert("ip".to_string(), if ip.is_empty() { "?".to_string() } else { ip });
            p.insert("count".to_string(), ids.len().to_string());
            p.insert("account".to_string(), accounts.into_iter().collect::<Vec<_>>().join(", "));
            p.insert("external".to_string(), external.to_string());
            let last = times.iter().max().copied();
            p.insert("time".to_string(), fmt_time(last));
            findings.push(Finding { rule_id: "R11".into(), level, params: p, evidence: ids, time: times.iter().min().copied() });
        }
    }

    // R12 하루 전송량이 많은 프로그램
    if r.large_upload && r.large_upload_mb > 0 {
        let limit = u64::from(r.large_upload_mb) * 1024 * 1024;
        let mut hits: Vec<(u64, usize)> = ev
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == EventKind::NetworkUsage && ctx.in_period(e.time.utc))
            .filter_map(|(i, e)| Some((detail(e, "bytes_sent")?.parse::<u64>().ok()?, i)))
            .filter(|(b, _)| *b >= limit)
            .collect();
        hits.sort_by_key(|x| std::cmp::Reverse(x.0));
        for (bytes, i) in hits.into_iter().take(10) {
            let Some(e) = ev.get(i) else { continue };
            let mut p = BTreeMap::new();
            p.insert("program".to_string(), e.subject.clone());
            p.insert("size".to_string(), human_bytes(bytes));
            p.insert("day".to_string(), detail(e, "day_utc").unwrap_or("").to_string());
            p.insert("time".to_string(), fmt_time(e.time.utc));
            findings.push(Finding { rule_id: "R12".into(), level: Level::Medium, params: p, evidence: vec![i], time: e.time.utc });
        }
    }

    // R13 원격 제어 프로그램 실행
    if r.remote_tools {
        let names: BTreeSet<String> = r.remote_tool_names.iter().map(|n| n.trim().to_ascii_lowercase()).collect();
        let mut by_prog: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in ev.iter().enumerate() {
            if e.kind == EventKind::ProgramRun && ctx.in_period(e.time.utc) && names.contains(&base_name(&e.subject)) {
                by_prog.entry(base_name(&e.subject)).or_default().push(i);
            }
        }
        for (prog, ids) in by_prog {
            let times: Vec<DateTime<Utc>> = ids.iter().filter_map(|&i| ev.get(i)?.time.utc).collect();
            let after_hours = times.iter().any(|t| ctx.after_hours(*t));
            let mut p = BTreeMap::new();
            p.insert("program".to_string(), prog);
            p.insert("count".to_string(), ids.len().to_string());
            p.insert("time".to_string(), fmt_time(times.iter().max().copied()));
            let level = if after_hours { Level::Medium } else { Level::Low };
            findings.push(Finding { rule_id: "R13".into(), level, params: p, evidence: ids, time: times.iter().min().copied() });
        }
    }

    findings.sort_by(|a, b| b.level.cmp(&a.level).then(a.time.cmp(&b.time)).then(a.rule_id.cmp(&b.rule_id)));

    // --- 등급 ---
    let mut missing_required = Vec::new();
    if !a.coverage.iter().any(|c| c.artifact == "hive_system" && c.files > 0) {
        missing_required.push("hive_system".to_string());
    }
    let max = findings.iter().map(|f| f.level).max();
    let grade = match max {
        Some(Level::High) => Grade::Review,
        Some(Level::Medium) => Grade::Caution,
        _ if !missing_required.is_empty() => Grade::Undetermined,
        _ => Grade::NothingFound,
    };

    Assessment {
        grade,
        findings,
        incidents,
        tool_media_events,
        missing_required,
        period_start: ctx.period_start,
        period_end: ctx.period_end,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use st_core::{AccountRef, Confidence, Coverage, SourceRef, Stamp};

    fn e(kind: EventKind, t: DateTime<Utc>, subject: &str, details: &[(&str, &str)]) -> Event {
        Event {
            time: Stamp::utc(Some(t), ""),
            kind,
            subject: subject.to_string(),
            account: AccountRef::default(),
            details: details.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            source: SourceRef { artifact: "x".into(), file: "f".into(), record: String::new() },
            confidence: Confidence::High,
            caveats: vec![],
        }
    }

    fn analysis(events: Vec<Event>) -> Analysis {
        Analysis {
            system: Default::default(),
            accounts: vec![],
            devices: vec![],
            networks: vec![],
            events,
            issues: vec![],
            coverage: vec![Coverage { artifact: "hive_system".into(), files: 1, events: 0, earliest: None, latest: None }],
        }
    }

    fn ctx<'a>(rules: &'a RuleConfig, cust: &'a Customer, tool: &'a ToolMedia) -> Context<'a> {
        Context { rules, customer: cust, tz: chrono_tz::Asia::Seoul, period_start: None, period_end: None, tool_media: tool, drives: &[] }
    }

    #[test]
    fn clean_input_is_nothing_found() {
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let a = assess(&analysis(vec![]), &ctx(&rules, &cust, &tool));
        assert_eq!(a.grade, Grade::NothingFound);
        let mut no_hive = analysis(vec![]);
        no_hive.coverage.clear();
        assert_eq!(assess(&no_hive, &ctx(&rules, &cust, &tool)).grade, Grade::Undetermined);
    }

    #[test]
    fn removable_open_linked_by_vsn_is_review() {
        // 2026-11-03(화) 22:12 KST = 13:12 UTC → 업무 시간 외
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 13, 12, 0).unwrap();
        let events = vec![
            e(EventKind::StorageAttached, t, "SanDisk Ultra", &[("volume_serial", "1A2B-3C4D"), ("device_serial", "4C53"), ("connected", "true")]),
            e(EventKind::FileOpened, t + Duration::minutes(2), "E:\\자료\\a.docx", &[("drive_type", "removable"), ("volume_serial", "1A2B-3C4D")]),
        ];
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(events), &ctx(&rules, &cust, &tool));
        assert_eq!(r.grade, Grade::Review);
        assert_eq!(r.incidents.len(), 1);
        assert_eq!(r.incidents[0].link, LinkStrength::Direct);
        assert!(r.findings.iter().any(|f| f.rule_id == "R3" && f.level == Level::High));
        assert!(r.findings.iter().any(|f| f.rule_id == "R2"));

        // 회사 장치로 등록하면 판단에서 빠진다
        let mut cust2 = Customer::default();
        cust2.allowed_devices.push(st_core::settings::AllowedDevice { id: "1a2b-3c4d".into(), memo: String::new() });
        cust2.allowed_devices.push(st_core::settings::AllowedDevice { id: "4C53".into(), memo: String::new() });
        let r2 = assess(&analysis(vec![
            e(EventKind::StorageAttached, t, "SanDisk Ultra", &[("volume_serial", "1A2B-3C4D"), ("device_serial", "4C53"), ("connected", "true")]),
            e(EventKind::FileOpened, t, "E:\\a.docx", &[("drive_type", "removable"), ("volume_serial", "1A2B-3C4D")]),
        ]), &ctx(&rules, &cust2, &tool));
        assert_eq!(r2.grade, Grade::NothingFound, "{:?}", r2.findings);

        // 점검 도구 매체도 빠진다
        let tool3 = ToolMedia { drive_letter: Some("E:".into()), volume_serial: Some("1A2B-3C4D".into()) };
        let r3 = assess(&analysis(vec![
            e(EventKind::StorageAttached, t, "SanDisk Ultra", &[("volume_serial", "1A2B-3C4D"), ("connected", "true")]),
            e(EventKind::FileOpened, t, "E:\\a.docx", &[("drive_type", "removable"), ("volume_serial", "1A2B-3C4D")]),
        ]), &ctx(&rules, &cust, &tool3));
        assert_eq!(r3.grade, Grade::NothingFound, "{:?}", r3.findings);
        assert_eq!(r3.tool_media_events.len(), 2);
    }

    #[test]
    fn time_change_only_counts_people() {
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 1, 0, 0).unwrap();
        let mut sys = e(EventKind::TimeChanged, t, "x", &[("delta_seconds", "25200")]);
        sys.account.sid = Some("S-1-5-19".into());
        let mut person = e(EventKind::TimeChanged, t, "x", &[("delta_seconds", "-259200")]);
        person.account.sid = Some("S-1-5-21-1-2-3-1001".into());
        let mut small = e(EventKind::TimeChanged, t, "x", &[("delta_seconds", "30")]);
        small.account.sid = Some("S-1-5-21-1-2-3-1001".into());
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(vec![sys, person, small]), &ctx(&rules, &cust, &tool));
        let r6: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R6").collect();
        assert_eq!(r6.len(), 1);
        assert_eq!(r6[0].params["minutes"], "-4320");
    }

    #[test]
    fn mass_delete_window() {
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 5, 0, 0).unwrap();
        let mut events: Vec<Event> = (0..25).map(|i| e(EventKind::FileDeleted, t + Duration::minutes(i), "f", &[])).collect();
        events.push(e(EventKind::FileDeleted, t + Duration::hours(5), "g", &[]));
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(events), &ctx(&rules, &cust, &tool));
        let r4: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R4").collect();
        assert_eq!(r4.len(), 1, "{:?}", r4);
        assert_eq!(r4[0].params["count"], "25");
        assert_eq!(r.grade, Grade::Caution);
    }

    #[test]
    fn cloud_domains_and_period() {
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 5, 0, 0).unwrap();
        let events = vec![
            e(EventKind::WebVisit, t, "https://drive.google.com/drive/my-drive", &[]),
            e(EventKind::WebVisit, t, "https://evil-drive.google.com.example.org/", &[]),
            e(EventKind::WebVisit, t, "https://mail.naver.com/", &[]),
            e(EventKind::WebVisit, t - Duration::days(100), "https://dropbox.com/", &[]),
        ];
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let mut c = ctx(&rules, &cust, &tool);
        c.period_start = Some(t - Duration::days(30));
        let r = assess(&analysis(events), &c);
        let f = r.findings.iter().find(|f| f.rule_id == "R8").unwrap();
        assert_eq!(f.params["count"], "2");
        assert_eq!(f.level, Level::Low);
        assert_eq!(r.grade, Grade::NothingFound, "참고(Low)만 있으면 등급을 올리지 않는다");
    }

    #[test]
    fn program_from_removable_and_tools() {
        // 업무 시간(화 10:00 KST = 01:00 UTC)
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 1, 0, 0).unwrap();
        let events = vec![
            e(EventKind::StorageAttached, t, "Kingston", &[("volume_serial", "AAAA-BBBB"), ("connected", "true")]),
            e(EventKind::ProgramRun, t, "[AAAA-BBBB]\\TOOLS\\COPYALL.EXE", &[("volume_serial", "AAAA-BBBB")]),
            e(EventKind::ProgramRun, t, "C:\\Windows\\System32\\notepad.exe", &[("volume_serial", "1111-2222")]),
            e(EventKind::ProgramRun, t, "C:\\Program Files\\BleachBit\\bleachbit.exe", &[]),
            e(EventKind::ProgramRun, t, "C:\\Program Files\\AnyDesk\\AnyDesk.exe", &[]),
        ];
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(events), &ctx(&rules, &cust, &tool));
        let r9: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R9").collect();
        assert_eq!(r9.len(), 1, "{:?}", r.findings);
        assert!(r9[0].params["program"].contains("COPYALL"));
        assert_eq!(r9[0].level, Level::Medium);
        assert!(r.findings.iter().any(|f| f.rule_id == "R10" && f.params["program"] == "bleachbit.exe" && f.level == Level::High));
        assert!(r.findings.iter().any(|f| f.rule_id == "R13" && f.params["program"] == "anydesk.exe" && f.level == Level::Low));
        assert_eq!(r.grade, Grade::Review);

        // 이동식 드라이브 문자로도 판단한다 (점검 시점 드라이브 정보)
        let drives = vec![DriveInfo { letter: "F:".into(), device: String::new(), serial: None, removable: true }];
        let mut c2 = ctx(&rules, &cust, &tool);
        c2.drives = &drives;
        let r2 = assess(&analysis(vec![e(EventKind::ProgramRun, t, "F:\\run.exe", &[])]), &c2);
        assert!(r2.findings.iter().any(|f| f.rule_id == "R9"));
        // 점검 도구가 있는 드라이브는 뺀다
        let tool_f = ToolMedia { drive_letter: Some("F:".into()), volume_serial: None };
        let mut c3 = ctx(&rules, &cust, &tool_f);
        c3.drives = &drives;
        let r3 = assess(&analysis(vec![e(EventKind::ProgramRun, t, "F:\\SecuridgeTrace.exe", &[])]), &c3);
        assert!(!r3.findings.iter().any(|f| f.rule_id == "R9"), "{:?}", r3.findings);
    }

    #[test]
    fn remote_logons_and_uploads() {
        let t = Utc.with_ymd_and_hms(2026, 11, 3, 1, 0, 0).unwrap();
        let mut ext = e(EventKind::RemoteLogon, t, "203.0.113.7", &[("action", "logon"), ("source_ip", "203.0.113.7")]);
        ext.account.name = Some("kim".into());
        let lan = e(EventKind::RemoteLogon, t, "192.168.0.5", &[("action", "auth"), ("source_ip", "192.168.0.5")]);
        let disc = e(EventKind::RemoteLogon, t, "192.168.0.5", &[("action", "disconnect"), ("source_ip", "192.168.0.5")]);
        let big = e(EventKind::NetworkUsage, t, "C:\\x\\uploader.exe", &[("bytes_sent", "2147483648"), ("day_utc", "2026-11-03")]);
        let small = e(EventKind::NetworkUsage, t, "C:\\x\\chat.exe", &[("bytes_sent", "1048576")]);
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(vec![ext, lan, disc, big, small]), &ctx(&rules, &cust, &tool));
        let r11: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R11").collect();
        assert_eq!(r11.len(), 2);
        let e11 = r11.iter().find(|f| f.params["ip"] == "203.0.113.7").unwrap();
        assert_eq!(e11.level, Level::High);
        assert_eq!(e11.params["account"], "kim");
        let l11 = r11.iter().find(|f| f.params["ip"] == "192.168.0.5").unwrap();
        assert_eq!(l11.level, Level::Low);
        assert_eq!(l11.params["count"], "1", "연결 끊김은 세지 않는다");
        let r12: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R12").collect();
        assert_eq!(r12.len(), 1);
        assert_eq!(r12[0].params["size"], "2.0 GB");
        assert!(private_ip("10.1.2.3") && private_ip("fe80::1") && !private_ip("8.8.8.8"));
    }

    #[test]
    fn internal_disks_are_not_storage_use() {
        // 일요일 03:00 KST (업무 시간 밖)
        let t = Utc.with_ymd_and_hms(2026, 11, 21, 18, 0, 0).unwrap();
        let fixed = e(EventKind::StorageAttached, t, "Msft Virtual Disk", &[("connected", "true"), ("removable", "false"), ("volume_serial", "4CDA-EECE")]);
        let usb = e(EventKind::StorageAttached, t, "SanDisk", &[("connected", "true"), ("removable", "true"), ("volume_serial", "1111-2222")]);
        let run_c = e(EventKind::ProgramRun, t, "C:\\Windows\\notepad.exe", &[("volume_serial", "4CDA-EECE")]);
        let rules = RuleConfig::default();
        let cust = Customer::default();
        let tool = ToolMedia::default();
        let r = assess(&analysis(vec![fixed, usb, run_c]), &ctx(&rules, &cust, &tool));
        let r2: Vec<_> = r.findings.iter().filter(|f| f.rule_id == "R2").collect();
        assert_eq!(r2.len(), 1, "{:?}", r.findings);
        assert_eq!(r2[0].params["device"], "SanDisk");
        assert!(!r.findings.iter().any(|f| f.rule_id == "R9"), "내장 디스크에서 실행한 프로그램은 R9가 아니다");
    }
}
