//! st-timeline: 찾은 파일을 파서에 넘겨 사실(Event)로 바꾸고 시간순으로 합친다.
//!
//! 지금은 단순 병합만 한다. 사실들 사이의 연결(USB ↔ 파일 열람 ↔ 삭제)을 만드는
//! 상관 분석은 Phase 4 과제다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use encoding_rs::Encoding;
use serde::Serialize;
use st_collect::{
    sqlite_sidecars, FoundItem, Scan, ARTIFACT_CHROMIUM, ARTIFACT_FIREFOX, ARTIFACT_LNK,
    ARTIFACT_RECYCLE_BIN,
};
use st_core::{Confidence, Coverage, Event, EventKind, Issue, SourceRef, Stamp};
use st_parsers::{browser, lnk, recyclebin};

/// 파일 종류별로 메모리에 올리는 최대 크기.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_LNK_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub events: Vec<Event>,
    pub issues: Vec<Issue>,
    pub coverage: Vec<Coverage>,
}

pub struct Options<'a> {
    /// 브라우저 DB 사본을 잠시 둘 작업 폴더 (입력 폴더 밖이어야 한다).
    pub work_dir: &'a Path,
    /// 대상 PC의 ANSI 코드 페이지 (유니코드 경로가 없는 바로 가기에만 쓰인다).
    pub ansi: &'static Encoding,
}

/// 찾은 파일 전부를 분석한다. 어떤 파일이 실패해도 멈추지 않고 `issues`에 남긴다.
pub fn analyze(scan: &Scan, opts: &Options) -> Analysis {
    let mut events: Vec<Event> = Vec::new();
    let mut issues: Vec<Issue> = scan.issues.clone();

    for (index, item) in scan.items.iter().enumerate() {
        let result = match item.artifact.as_str() {
            ARTIFACT_RECYCLE_BIN => recycle_events(item),
            ARTIFACT_LNK => lnk_events(item, opts.ansi),
            ARTIFACT_CHROMIUM | ARTIFACT_FIREFOX => browser_events(item, opts.work_dir, index),
            other => Err(format!("처리 방법이 없는 종류: {other}")),
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

    // 시각이 없는 사실은 맨 뒤로. 나머지 기준은 실행할 때마다 순서가 같도록 하기 위한 것이다.
    events.sort_by(|a, b| {
        (a.time.utc.is_none(), &a.time.utc, &a.kind, &a.source, &a.subject)
            .cmp(&(b.time.utc.is_none(), &b.time.utc, &b.kind, &b.source, &b.subject))
    });
    issues.sort();

    let coverage = coverage(scan, &events);
    Analysis { events, issues, coverage }
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
        caveats: vec![
            "휴지통으로 옮긴 기록임. 휴지통을 비운 뒤에는 이 기록도 사라짐.".to_string(),
        ],
    };
    Ok((vec![ev], Vec::new()))
}

fn lnk_events(item: &FoundItem, ansi: &'static Encoding) -> Result<EventsAndNotes, String> {
    let data = read_limited(&item.abs_path, MAX_LNK_BYTES)?;
    let info = lnk::parse(&data, ansi).map_err(|e| e.to_string())?;
    let Some(target) = info.target_path() else {
        // 제어판 항목처럼 파일 경로가 없는 바로 가기. 오류가 아니라 대상 밖이다.
        return Ok((Vec::new(), vec!["대상 경로가 없는 바로 가기라 건너뜀".to_string()]));
    };

    // 바로 가기 파일 자체의 수정 시각 = 대상을 마지막으로 연 시각(으로 알려져 있음).
    // 다만 이 값은 파일 시스템의 값이라, 수집할 때 보존하지 않았다면 수집 시각으로 바뀌어 있다.
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
        // 시각이 수집 방식에 좌우되므로 '중간'.
        confidence: Confidence::Medium,
        caveats: vec![
            "파일을 '열었다'는 흔적이며 '복사했다'는 증거가 아님.".to_string(),
            "시각은 바로 가기 파일의 수정 시각임. 수집 과정에서 파일 시각을 보존하지 않았다면 실제와 다름."
                .to_string(),
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
        // -shm은 SQLite가 다시 만들 수 있으므로 복사하지 않는다.
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

fn coverage(scan: &Scan, events: &[Event]) -> Vec<Coverage> {
    scan.checks
        .iter()
        .map(|check| {
            let times: Vec<DateTime<Utc>> = events
                .iter()
                .filter(|e| e.source.artifact == check.artifact)
                .filter_map(|e| e.time.utc)
                .collect();
            Coverage {
                artifact: check.artifact.clone(),
                files: check.count,
                events: events.iter().filter(|e| e.source.artifact == check.artifact).count(),
                earliest: times.iter().min().copied(),
                latest: times.iter().max().copied(),
            }
        })
        .collect()
}
