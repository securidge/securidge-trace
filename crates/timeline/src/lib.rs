//! st-timeline: 찾은 파일을 파서에 넘겨 사실(Event)로 바꾸고 시간순으로 합친다.
//!
//! 순서
//! 1. 레지스트리에서 시스템 정보(시간대, 코드 페이지)와 계정 목록을 먼저 읽는다.
//!    → 로컬 시각 변환, 바로 가기 해석, SID ↔ 계정 이름 연결에 쓴다.
//! 2. 장치·네트워크·이벤트 로그·휴지통·바로 가기·브라우저 기록을 사실로 바꾼다.
//! 3. 시간순으로 정렬하고, 아티팩트별 데이터 범위를 계산한다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use st_collect::{
    sqlite_sidecars, FoundItem, Scan, ARTIFACT_CHROMIUM, ARTIFACT_EVTX_PARTITION,
    ARTIFACT_EVTX_SECURITY, ARTIFACT_EVTX_SYSTEM, ARTIFACT_EVTX_WLAN, ARTIFACT_FIREFOX,
    ARTIFACT_HIVE_SOFTWARE, ARTIFACT_HIVE_SYSTEM, ARTIFACT_LNK, ARTIFACT_RECYCLE_BIN,
};
use st_core::{AccountRef, Confidence, Coverage, Event, EventKind, Issue, SourceRef, Stamp, TimeBasis};
use st_parsers::hive::Hive;
use st_parsers::registry::{self, Account, Device, DeviceKind, Network, SystemInfo};
use st_parsers::{browser, eventlog, lnk, recyclebin};

/// (가장 오래된 기록, 가장 최근 기록)
type Range = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_LNK_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub system: SystemInfo,
    pub accounts: Vec<Account>,
    pub devices: Vec<Device>,
    pub networks: Vec<Network>,
    pub events: Vec<Event>,
    pub issues: Vec<Issue>,
    pub coverage: Vec<Coverage>,
}

pub struct Options<'a> {
    /// 브라우저 DB 사본을 잠시 둘 작업 폴더 (입력 폴더 밖이어야 한다).
    pub work_dir: &'a Path,
    /// 레지스트리에서 코드 페이지를 못 읽었을 때 쓸 값.
    pub fallback_ansi: &'static Encoding,
}

/// 코드 페이지 번호 → 인코딩 (자주 쓰는 것만).
fn encoding_for(cp: u32) -> Option<&'static Encoding> {
    Some(match cp {
        949 => encoding_rs::EUC_KR,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        950 => encoding_rs::BIG5,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1252 => encoding_rs::WINDOWS_1252,
        _ => return None,
    })
}

/// 찾은 파일 전부를 분석한다. 어떤 파일이 실패해도 멈추지 않고 `issues`에 남긴다.
pub fn analyze(scan: &Scan, opts: &Options) -> Analysis {
    let mut events: Vec<Event> = Vec::new();
    let mut issues: Vec<Issue> = scan.issues.clone();
    let mut extra_ranges: BTreeMap<String, Range> =
        BTreeMap::new();

    let find = |a: &str| scan.items.iter().find(|i| i.artifact == a);

    // --- 1. 레지스트리 ---
    let mut system = SystemInfo::default();
    let mut accounts = Vec::new();
    let mut devices = Vec::new();
    let mut networks = Vec::new();

    let mut software = match find(ARTIFACT_HIVE_SOFTWARE) {
        Some(item) => match Hive::open(&item.abs_path) {
            Ok(h) => {
                if let Some(n) = &h.log_note {
                    issues.push(issue(item, n.clone()).into_note());
                }
                Some(h)
            }
            Err(e) => {
                issues.push(issue(item, e));
                None
            }
        },
        None => None,
    };

    if let Some(item) = find(ARTIFACT_HIVE_SYSTEM) {
        match Hive::open(&item.abs_path) {
            Ok(mut h) => {
                if let Some(n) = &h.log_note {
                    issues.push(issue(item, n.clone()).into_note());
                }
                system = registry::system_info(&mut h, software.as_mut());
                devices = registry::usb_devices(&mut h);
                devices.extend(registry::bluetooth_devices(&mut h));
                for d in &devices {
                    events.extend(device_events(item, d));
                }
            }
            Err(e) => issues.push(issue(item, e)),
        }
    }
    if let (Some(item), Some(sw)) = (find(ARTIFACT_HIVE_SOFTWARE), software.as_mut()) {
        accounts = registry::accounts(sw);
        networks = registry::networks(sw, system.utc_offset_minutes);
        for n in &networks {
            events.extend(network_events(item, n, &system));
        }
    }

    let ansi = system.ansi_codepage.and_then(encoding_for).unwrap_or(opts.fallback_ansi);

    // --- 2. 이벤트 로그 ---
    for artifact in [ARTIFACT_EVTX_SECURITY, ARTIFACT_EVTX_SYSTEM, ARTIFACT_EVTX_PARTITION, ARTIFACT_EVTX_WLAN] {
        let Some(item) = find(artifact) else { continue };
        match eventlog::read(&item.abs_path) {
            Ok(read) => {
                extra_ranges.insert(artifact.to_string(), (read.earliest, read.latest));
                if read.bad_records > 0 {
                    issues.push(issue(item, format!("읽지 못한 레코드 {}개", read.bad_records)));
                }
                for le in read.events {
                    if let Some(ev) = log_event(item, &le) {
                        events.push(ev);
                    }
                }
            }
            Err(e) => issues.push(issue(item, e)),
        }
    }

    // --- 3. 파일 기반 기록 ---
    for (index, item) in scan.items.iter().enumerate() {
        let result = match item.artifact.as_str() {
            ARTIFACT_RECYCLE_BIN => recycle_events(item),
            ARTIFACT_LNK => lnk_events(item, ansi),
            ARTIFACT_CHROMIUM | ARTIFACT_FIREFOX => browser_events(item, opts.work_dir, index),
            _ => continue,
        };
        match result {
            Ok((mut ev, notes)) => {
                events.append(&mut ev);
                for n in notes {
                    issues.push(issue(item, n));
                }
            }
            Err(reason) => issues.push(issue(item, reason)),
        }
    }

    // --- 4. 계정 이름 채우기 (SID ↔ 프로필 폴더 이름) ---
    for e in &mut events {
        fill_account(&mut e.account, &accounts);
    }

    events.sort_by(|a, b| {
        (a.time.utc.is_none(), &a.time.utc, &a.kind, &a.source, &a.subject)
            .cmp(&(b.time.utc.is_none(), &b.time.utc, &b.kind, &b.source, &b.subject))
    });
    issues.sort();

    let coverage = coverage(scan, &events, &extra_ranges);
    Analysis { system, accounts, devices, networks, events, issues, coverage }
}

fn fill_account(a: &mut AccountRef, accounts: &[Account]) {
    if a.name.is_none() {
        if let Some(sid) = &a.sid {
            a.name = accounts.iter().find(|x| x.sid.eq_ignore_ascii_case(sid)).map(|x| x.name.clone());
        }
    }
    if a.sid.is_none() {
        if let Some(name) = &a.name {
            a.sid = accounts.iter().find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.sid.clone());
        }
    }
}

trait AsNote {
    fn into_note(self) -> Issue;
}
impl AsNote for Issue {
    /// 실패는 아니지만 알아 둘 내용. reason 앞에 "참고:"를 붙인다.
    fn into_note(mut self) -> Issue {
        self.reason = format!("참고: {}", self.reason);
        self
    }
}

fn issue(item: &FoundItem, reason: String) -> Issue {
    Issue { artifact: item.artifact.clone(), file: item.rel_path.clone(), reason }
}

fn source(item: &FoundItem, record: String) -> SourceRef {
    SourceRef { artifact: item.artifact.clone(), file: item.rel_path.clone(), record }
}

type EventsAndNotes = (Vec<Event>, Vec<String>);

fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|e| format!("파일 정보를 읽지 못함: {e}"))?;
    if meta.len() > max {
        return Err(format!("파일이 너무 큼 ({}바이트, 상한 {max})", meta.len()));
    }
    fs::read(path).map_err(|e| format!("파일을 읽지 못함: {e}"))
}

fn iso(dt: Option<DateTime<Utc>>) -> String {
    dt.map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn device_label(d: &Device) -> String {
    let parts: Vec<&str> = [d.vendor.as_deref(), d.product.as_deref()].into_iter().flatten().collect();
    if !parts.is_empty() {
        parts.join(" ")
    } else {
        d.friendly_name.clone().unwrap_or_else(|| "알 수 없는 장치".to_string())
    }
}

fn device_events(item: &FoundItem, d: &Device) -> Vec<Event> {
    let mut details = BTreeMap::new();
    details.insert("device_kind".to_string(), format!("{:?}", d.kind));
    if let Some(s) = &d.serial {
        details.insert("device_serial".to_string(), s.clone());
    }
    details.insert("serial_is_unique".to_string(), d.serial_is_unique.to_string());
    if let Some(f) = &d.friendly_name {
        details.insert("friendly_name".to_string(), f.clone());
    }
    if !d.drive_letters.is_empty() {
        details.insert("drive_letters".to_string(), d.drive_letters.join(","));
    }
    let mut caveats = Vec::new();
    if !d.serial_is_unique {
        caveats.push("장치가 고유 일련번호를 알려주지 않아, 다른 PC의 기록과 같은 장치인지 확인할 수 없음.".to_string());
    }
    if d.kind == DeviceKind::Portable {
        caveats.push("스마트폰 등 휴대 장치는 어떤 파일을 옮겼는지 기록이 거의 남지 않음.".to_string());
    }
    let confidence = if d.kind == DeviceKind::Bluetooth { Confidence::Low } else { Confidence::High };
    let label = device_label(d);
    let mut out = Vec::new();
    for (kind, t, rec) in [
        (EventKind::DeviceFirstConnected, d.first_install, "0064"),
        (EventKind::DeviceLastConnected, d.last_arrival, "0066"),
        (EventKind::DeviceLastRemoved, d.last_removal, "0067"),
    ] {
        if t.is_none() && !(kind == EventKind::DeviceLastConnected && d.kind == DeviceKind::Bluetooth) {
            continue;
        }
        let kind = if d.kind == DeviceKind::Bluetooth { EventKind::BluetoothConnected } else { kind };
        if d.kind == DeviceKind::Bluetooth && t.is_none() {
            continue;
        }
        out.push(Event {
            time: Stamp::utc(t, iso(t)),
            kind,
            subject: label.clone(),
            account: AccountRef::default(),
            details: details.clone(),
            source: source(item, format!("{}\\{rec}", d.key_path)),
            confidence,
            caveats: caveats.clone(),
        });
    }
    out
}

fn network_events(item: &FoundItem, n: &Network, sys: &SystemInfo) -> Vec<Event> {
    let mut details = BTreeMap::new();
    details.insert("medium".to_string(), n.medium.clone());
    if let Some(m) = &n.gateway_mac {
        details.insert("gateway_mac".to_string(), m.clone());
    }
    if let Some(d) = &n.description {
        details.insert("description".to_string(), d.clone());
    }
    let mut caveats = vec!["원본 시각은 대상 PC의 로컬 시간이며, 대상 PC의 시간대 설정으로 UTC로 바꿨음.".to_string()];
    let mut confidence = Confidence::High;
    if sys.utc_offset_minutes.is_none() {
        caveats.push("대상 PC의 시간대를 몰라 UTC로 바꾸지 못함.".to_string());
        confidence = Confidence::Low;
    } else if sys.uses_dst {
        caveats.push("일광 절약 시간을 쓰는 시간대라 1시간 오차가 있을 수 있음.".to_string());
        confidence = Confidence::Medium;
    }
    let mut out = Vec::new();
    for (kind, utc, raw) in [
        (EventKind::NetworkFirstConnected, n.created_utc, &n.created_local_raw),
        (EventKind::NetworkLastConnected, n.last_connected_utc, &n.last_connected_local_raw),
    ] {
        let Some(raw) = raw else { continue };
        out.push(Event {
            time: Stamp { utc, raw: raw.clone(), basis: TimeBasis::Local },
            kind,
            subject: n.name.clone(),
            account: AccountRef::default(),
            details: details.clone(),
            source: source(item, format!("NetworkList\\Profiles\\{}", n.profile_guid)),
            confidence,
            caveats: caveats.clone(),
        });
    }
    out
}

fn log_event(item: &FoundItem, le: &eventlog::LogEvent) -> Option<Event> {
    let g = |k: &str| le.data.get(k).cloned().unwrap_or_default();
    let mut details: BTreeMap<String, String> = BTreeMap::new();
    details.insert("event_id".to_string(), le.event_id.to_string());
    details.insert("channel".to_string(), le.channel.clone());
    let mut account = AccountRef::default();
    let mut caveats = Vec::new();
    let (kind, subject) = match (le.channel.to_ascii_lowercase().as_str(), le.event_id) {
        ("security", 1102) => {
            account.sid = le.data.get("LogFileCleared.SubjectUserSid").cloned();
            details.insert("by_user".to_string(), g("LogFileCleared.SubjectUserName"));
            (EventKind::LogCleared, "Security".to_string())
        }
        ("system", 104) => {
            let channel = le
                .data
                .iter()
                .find(|(k, _)| k.ends_with("Channel"))
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "System".to_string());
            let user = le.data.iter().find(|(k, _)| k.ends_with("SubjectUserName")).map(|(_, v)| v.clone());
            if let Some(u) = user {
                details.insert("by_user".to_string(), u);
            }
            (EventKind::LogCleared, channel)
        }
        ("security", 4616) => {
            account.sid = le.data.get("SubjectUserSid").cloned();
            details.insert("previous_time".to_string(), g("PreviousTime"));
            details.insert("new_time".to_string(), g("NewTime"));
            details.insert("process".to_string(), g("ProcessName"));
            details.insert("by_user".to_string(), g("SubjectUserName"));
            if let Some(d) = time_delta(&g("PreviousTime"), &g("NewTime")) {
                details.insert("delta_seconds".to_string(), d.to_string());
            }
            (EventKind::TimeChanged, "시스템 시각".to_string())
        }
        ("system", 1) => {
            details.insert("previous_time".to_string(), g("OldTime"));
            details.insert("new_time".to_string(), g("NewTime"));
            details.insert("reason".to_string(), g("Reason"));
            if let Some(d) = time_delta(&g("OldTime"), &g("NewTime")) {
                details.insert("delta_seconds".to_string(), d.to_string());
            }
            (EventKind::TimeChanged, "시스템 시각".to_string())
        }
        (c, 1006) if c.contains("partition") => {
            for (k, name) in [
                ("Manufacturer", "manufacturer"),
                ("Model", "model"),
                ("SerialNumber", "device_serial"),
                ("Capacity", "capacity_bytes"),
                ("VolumeSerial", "volume_serial"),
                ("ParentId", "parent_id"),
            ] {
                if let Some(v) = le.data.get(k).filter(|v| !v.trim().is_empty()) {
                    details.insert(name.to_string(), v.trim().to_string());
                }
            }
            let label = [g("Manufacturer"), g("Model")]
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let cap = le.data.get("Capacity").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0);
            details.insert("connected".to_string(), (cap > 0).to_string());
            caveats.push("용량이 0이면 분리 기록, 0보다 크면 연결 기록으로 본다.".to_string());
            (EventKind::StorageAttached, if label.is_empty() { "저장장치".to_string() } else { label })
        }
        (c, 8001) if c.contains("wlan") => {
            details.insert("ssid".to_string(), g("SSID"));
            (EventKind::WlanConnected, g("ProfileName"))
        }
        (c, 8003) if c.contains("wlan") => {
            details.insert("ssid".to_string(), g("SSID"));
            details.insert("reason".to_string(), g("Reason"));
            (EventKind::WlanDisconnected, g("ProfileName"))
        }
        _ => return None,
    };
    Some(Event {
        time: Stamp::utc(le.time, iso(le.time)),
        kind,
        subject,
        account,
        details,
        source: source(item, format!("EventRecordID={}", le.record_id)),
        confidence: Confidence::High,
        caveats,
    })
}

fn time_delta(prev: &str, new: &str) -> Option<i64> {
    let p = DateTime::parse_from_rfc3339(prev).ok()?;
    let n = DateTime::parse_from_rfc3339(new).ok()?;
    Some((n - p).num_seconds())
}

fn recycle_events(item: &FoundItem) -> Result<EventsAndNotes, String> {
    let data = read_limited(&item.abs_path, MAX_INDEX_BYTES)?;
    let entry = recyclebin::parse_index(&data).map_err(|e| e.to_string())?;

    let mut details = BTreeMap::new();
    details.insert("size_bytes".to_string(), entry.original_size.to_string());
    details.insert("index_format_version".to_string(), entry.version.to_string());

    let ev = Event {
        time: Stamp::utc(entry.deleted_at, entry.deleted_at_raw.to_string()),
        kind: EventKind::FileDeleted,
        subject: entry.original_path,
        account: item.account.clone(),
        details,
        source: source(item, String::new()),
        confidence: Confidence::High,
        caveats: vec!["휴지통으로 옮긴 기록임. 휴지통을 비운 뒤에는 이 기록도 사라짐.".to_string()],
    };
    Ok((vec![ev], Vec::new()))
}

fn lnk_events(item: &FoundItem, ansi: &'static Encoding) -> Result<EventsAndNotes, String> {
    let data = read_limited(&item.abs_path, MAX_LNK_BYTES)?;
    let info = lnk::parse(&data, ansi).map_err(|e| e.to_string())?;
    let Some(target) = info.target_path() else {
        return Ok((Vec::new(), vec!["참고: 대상 경로가 없는 바로 가기라 건너뜀".to_string()]));
    };

    // 바로 가기 파일 자체의 수정 시각 = 대상을 마지막으로 연 시각(으로 알려져 있음).
    let modified: Option<DateTime<Utc>> = fs::metadata(&item.abs_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(system_time_to_utc);

    let mut details = BTreeMap::new();
    if let Some(v) = info.drive_type_name() {
        details.insert("drive_type".to_string(), v.to_string());
    }
    if let Some(v) = info.drive_serial_display() {
        details.insert("volume_serial".to_string(), v);
    }
    if let Some(v) = &info.volume_label {
        details.insert("volume_label".to_string(), v.clone());
    }
    details.insert("target_size_bytes".to_string(), info.target_size.to_string());
    details.insert("target_created_utc".to_string(), iso(info.target_created));
    details.insert("target_modified_utc".to_string(), iso(info.target_modified));
    details.insert("target_accessed_utc".to_string(), iso(info.target_accessed));

    let ev = Event {
        time: Stamp::file_system(modified, iso(modified)),
        kind: EventKind::FileOpened,
        subject: target,
        account: item.account.clone(),
        details,
        source: source(item, String::new()),
        confidence: Confidence::Medium,
        caveats: vec![
            "파일을 '열었다'는 흔적이며 '복사했다'는 증거가 아님.".to_string(),
            "시각은 바로 가기 파일의 수정 시각임. 자료를 복사해서 가져왔다면 실제와 다를 수 있음.".to_string(),
        ],
    };
    Ok((vec![ev], Vec::new()))
}

fn system_time_to_utc(t: SystemTime) -> Option<DateTime<Utc>> {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    DateTime::from_timestamp(i64::try_from(d.as_secs()).ok()?, d.subsec_nanos())
}

/// 브라우저 DB와 보조 파일을 작업 폴더로 복사한 뒤 사본을 연다. 끝나면 사본을 지운다.
fn browser_events(item: &FoundItem, work_dir: &Path, index: usize) -> Result<EventsAndNotes, String> {
    let copy_dir = work_dir.join(format!("db-{index}"));
    fs::create_dir_all(&copy_dir).map_err(|e| format!("작업 폴더를 만들지 못함: {e}"))?;
    let result = browser_events_in(item, &copy_dir);
    let _ = fs::remove_dir_all(&copy_dir);
    result
}

fn browser_events_in(item: &FoundItem, copy_dir: &Path) -> Result<EventsAndNotes, String> {
    let copy = copy_file_into(&item.abs_path, copy_dir)?;
    for side in sqlite_sidecars(&item.abs_path) {
        if side.to_string_lossy().ends_with("-shm") {
            continue;
        }
        copy_file_into(&side, copy_dir)?;
    }

    let is_chromium = item.artifact == ARTIFACT_CHROMIUM;
    let history =
        if is_chromium { browser::read_chromium(&copy)? } else { browser::read_firefox(&copy)? };

    let base_details = || {
        let mut d = BTreeMap::new();
        if let Some(b) = &item.browser {
            d.insert("browser".to_string(), b.clone());
        }
        if let Some(p) = &item.profile {
            d.insert("profile".to_string(), p.clone());
        }
        d
    };
    let visit_table = if is_chromium { "visits" } else { "moz_historyvisits" };

    let mut events = Vec::new();
    for v in history.visits {
        let mut details = base_details();
        if !v.title.is_empty() {
            details.insert("title".to_string(), v.title);
        }
        events.push(Event {
            time: Stamp::utc(v.time, v.time_raw.to_string()),
            kind: EventKind::WebVisit,
            subject: v.url,
            account: item.account.clone(),
            details,
            source: source(item, format!("{visit_table}.id={}", v.record_id)),
            confidence: Confidence::High,
            caveats: Vec::new(),
        });
    }
    for d in history.downloads {
        let mut details = base_details();
        details.insert("url".to_string(), d.url);
        details.insert("total_bytes".to_string(), d.total_bytes.to_string());
        details.insert("end_utc".to_string(), iso(d.end));
        if let Some(s) = d.state {
            details.insert("state".to_string(), s.to_string());
        }
        events.push(Event {
            time: Stamp::utc(d.start, d.start_raw.to_string()),
            kind: EventKind::WebDownload,
            subject: d.target_path,
            account: item.account.clone(),
            details,
            source: source(item, format!("downloads.id={}", d.record_id)),
            confidence: Confidence::High,
            caveats: Vec::new(),
        });
    }
    Ok((events, history.notes))
}

fn copy_file_into(src: &Path, dir: &Path) -> Result<PathBuf, String> {
    let name = src.file_name().ok_or_else(|| "파일 이름이 없음".to_string())?;
    let dst = dir.join(name);
    fs::copy(src, &dst).map_err(|e| format!("작업용 사본을 만들지 못함: {e}"))?;
    Ok(dst)
}

fn coverage(
    scan: &Scan,
    events: &[Event],
    extra: &BTreeMap<String, Range>,
) -> Vec<Coverage> {
    scan.checks
        .iter()
        .map(|check| {
            let times: Vec<DateTime<Utc>> = events
                .iter()
                .filter(|e| e.source.artifact == check.artifact)
                .filter_map(|e| e.time.utc)
                .collect();
            // 이벤트 로그는 "골라낸 사건"이 아니라 "파일 전체가 담은 기간"을 범위로 쓴다.
            let (earliest, latest) = match extra.get(&check.artifact) {
                Some(r) => *r,
                None => (times.iter().min().copied(), times.iter().max().copied()),
            };
            Coverage {
                artifact: check.artifact.clone(),
                files: check.count,
                events: events.iter().filter(|e| e.source.artifact == check.artifact).count(),
                earliest,
                latest,
            }
        })
        .collect()
}
