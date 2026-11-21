//! 점프 목록(Jump List) 파서 — 프로그램별 "최근 연 파일".
//!
//! 작업 표시줄에서 프로그램을 오른쪽 클릭하면 보이는 최근 항목 목록이다. 사용자 폴더의
//! `AppData\Roaming\Microsoft\Windows\Recent\AutomaticDestinations\<AppID>.automaticDestinations-ms`
//! 에 저장되며, Recent 폴더의 바로 가기(.lnk)가 지워져도 여기에는 남아 있는 경우가 많다.
//!
//! 구조
//! - automaticDestinations-ms: OLE 복합 파일([MS-CFB]). `DestList` 스트림에 항목별 경로·마지막
//!   사용 시각·사용 횟수·고정 여부가 있고, 숫자 이름의 스트림마다 바로 가기(.lnk) 하나가 들어 있다.
//! - customDestinations-ms: 프로그램이 직접 만든 목록. 바로 가기가 연달아 붙어 있다.
//! - 파일 이름의 AppID로 프로그램을 알 수 있다 (data/jumplist_appids.txt, 알려진 것만).
//!
//! 근거: [MS-CFB] Compound File Binary File Format, [MS-SHLLINK],
//!   libyal/dtformats "Jump lists format" 문서, Eric Zimmerman JumpList 라이브러리(MIT)의 DestList 위치 값.
//! 검증: tests/jumplist_samples.rs — 공개 표본 파일의 항목 수·경로·시각 비교.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use st_core::bytes;
use st_core::time::filetime_to_utc;

use crate::lnk;

const APPIDS: &str = include_str!("../data/jumplist_appids.txt");
const MAX_ENTRIES: usize = 10_000;
const MAX_STREAM_BYTES: u64 = 16 * 1024 * 1024;
const LNK_HEADER: [u8; 20] = [
    0x4C, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x46,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpEntry {
    pub entry_number: u32,
    /// DestList의 경로 (파일 경로 또는 URL)
    pub path: String,
    /// 이 항목을 마지막으로 쓴 시각
    pub last_used: Option<DateTime<Utc>>,
    pub last_used_raw: u64,
    /// 사용 횟수 (Windows 10 이후 형식만)
    pub interaction_count: Option<u32>,
    pub pinned: bool,
    /// 항목을 만든 컴퓨터 이름 (NetBIOS)
    pub hostname: String,
    pub drive_type: Option<String>,
    pub volume_serial: Option<String>,
    pub volume_label: Option<String>,
    pub target_size: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpList {
    pub app_id: String,
    pub app_name: Option<String>,
    pub version: Option<u32>,
    pub entries: Vec<JumpEntry>,
    pub notes: Vec<String>,
}

fn appid_table() -> &'static BTreeMap<String, String> {
    static T: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = BTreeMap::new();
        for line in APPIDS.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((id, name)) = line.split_once('|') else { continue };
            let id = id.trim_matches('"').trim().to_ascii_lowercase();
            let name = name.trim_matches('"').trim().to_string();
            if !id.is_empty() && !name.is_empty() {
                m.entry(id).or_insert(name);
            }
        }
        m
    })
}

/// AppID(16자리 16진수)로 프로그램 이름을 찾는다.
pub fn app_name(app_id: &str) -> Option<String> {
    appid_table().get(&app_id.trim().to_ascii_lowercase()).cloned()
}

/// 파일 이름(`<AppID>.automaticDestinations-ms`)에서 AppID를 꺼낸다.
pub fn app_id_from_file_name(name: &str) -> String {
    name.split('.').next().unwrap_or("").to_ascii_lowercase()
}

fn utf16(b: &[u8]) -> String {
    let u: Vec<u16> =
        b.chunks_exact(2).map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)])).collect();
    String::from_utf16_lossy(&u)
}

fn hostname(raw: &[u8]) -> String {
    // 보통 ASCII 16바이트, 일부는 UTF-16
    if raw.get(1) == Some(&0) && raw.first().is_some_and(|b| *b != 0) {
        return utf16(raw).split('\0').next().unwrap_or("").to_string();
    }
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).into_owned()
}

struct DestEntry {
    entry_number: u32,
    path: String,
    last_raw: u64,
    interaction: Option<u32>,
    pinned: bool,
    host: String,
}

fn parse_destlist(d: &[u8], notes: &mut Vec<String>) -> (Option<u32>, Vec<DestEntry>) {
    let Some(version) = bytes::u32_le(d, 0) else {
        notes.push("DestList가 비어 있음".into());
        return (None, Vec::new());
    };
    let count = bytes::u32_le(d, 4).unwrap_or(0);
    let path_off = if version > 1 { 128usize } else { 112usize };
    let mut out = Vec::new();
    let mut pos = 32usize;
    while pos < d.len() && out.len() < MAX_ENTRIES {
        if version == 1 && out.len() >= usize::try_from(count).unwrap_or(0) {
            break;
        }
        let Some(chars) = bytes::u16_le(d, pos + path_off) else { break };
        let entry_size = path_off + 2 + usize::from(chars) * 2;
        let Some(entry) = bytes::slice(d, pos, entry_size) else {
            notes.push(format!("DestList 항목이 잘림 (위치 0x{pos:X})"));
            break;
        };
        let mut stride = entry_size;
        if version > 1 {
            let Some(sps) = bytes::u32_le(d, pos + entry_size) else { break };
            let Some(s) = usize::try_from(sps).ok().and_then(|s| s.checked_add(entry_size + 4)) else { break };
            if pos + s > d.len() {
                notes.push(format!("DestList 속성 영역이 잘림 (위치 0x{pos:X})"));
                break;
            }
            stride = s;
        }
        let pin = bytes::u32_le(entry, 108).unwrap_or(0xFFFF_FFFF);
        out.push(DestEntry {
            entry_number: bytes::u32_le(entry, 88).unwrap_or(0),
            path: bytes::slice(entry, path_off + 2, usize::from(chars) * 2).map(utf16).unwrap_or_default(),
            last_raw: bytes::u64_le(entry, 100).unwrap_or(0),
            interaction: if version > 1 { bytes::u32_le(entry, 116) } else { None },
            pinned: pin != 0xFFFF_FFFF,
            host: bytes::slice(entry, 72, 16).map(hostname).unwrap_or_default(),
        });
        pos += stride;
    }
    (Some(version), out)
}

fn with_lnk(mut e: JumpEntry, data: Option<&[u8]>, ansi: &'static Encoding) -> JumpEntry {
    let Some(data) = data else { return e };
    let Ok(info) = lnk::parse(data, ansi) else { return e };
    if e.path.is_empty() || e.path.starts_with("::") || e.path.starts_with("knownfolder") {
        if let Some(t) = info.target_path() {
            e.path = t;
        }
    }
    e.drive_type = info.drive_type_name().map(str::to_string);
    e.volume_serial = info.drive_serial_display();
    e.volume_label = info.volume_label.clone();
    e.target_size = Some(info.target_size);
    e
}

/// automaticDestinations-ms 파일 하나를 해석한다.
pub fn parse_automatic(data: &[u8], app_id: &str, ansi: &'static Encoding) -> Result<JumpList, String> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| format!("복합 파일 형식이 아님: {e}"))?;
    let mut notes = Vec::new();
    let mut streams: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let names: Vec<String> = cf
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_string_lossy().trim_start_matches('/').to_string())
        .take(MAX_ENTRIES + 1)
        .collect();
    for name in names {
        let Ok(s) = cf.open_stream(format!("/{name}")) else { continue };
        let mut buf = Vec::new();
        if s.take(MAX_STREAM_BYTES).read_to_end(&mut buf).is_ok() {
            streams.insert(name.to_ascii_lowercase(), buf);
        }
    }
    let (version, dest) = match streams.get("destlist") {
        Some(d) => parse_destlist(d, &mut notes),
        None => {
            notes.push("DestList 스트림이 없음".into());
            (None, Vec::new())
        }
    };
    let entries = dest
        .into_iter()
        .map(|d| {
            let e = JumpEntry {
                entry_number: d.entry_number,
                path: d.path,
                last_used: filetime_to_utc(d.last_raw),
                last_used_raw: d.last_raw,
                interaction_count: d.interaction,
                pinned: d.pinned,
                hostname: d.host,
                drive_type: None,
                volume_serial: None,
                volume_label: None,
                target_size: None,
            };
            let key = format!("{:x}", d.entry_number);
            with_lnk(e, streams.get(&key).map(Vec::as_slice), ansi)
        })
        .collect();
    Ok(JumpList { app_id: app_id.to_ascii_lowercase(), app_name: app_name(app_id), version, entries, notes })
}

/// customDestinations-ms 파일 하나를 해석한다. 시각 정보가 없으므로 경로만 돌려준다.
pub fn parse_custom(data: &[u8], app_id: &str, ansi: &'static Encoding) -> Result<JumpList, String> {
    if data.len() < 24 {
        return Err("항목이 없는 목록".into());
    }
    let mut entries = Vec::new();
    let mut i = 0usize;
    while i + LNK_HEADER.len() <= data.len() && entries.len() < MAX_ENTRIES {
        if data.get(i..i + LNK_HEADER.len()) == Some(&LNK_HEADER[..]) {
            if let Some(rest) = data.get(i..) {
                let e = JumpEntry {
                    entry_number: u32::try_from(entries.len()).unwrap_or(u32::MAX),
                    path: String::new(),
                    last_used: None,
                    last_used_raw: 0,
                    interaction_count: None,
                    pinned: false,
                    hostname: String::new(),
                    drive_type: None,
                    volume_serial: None,
                    volume_label: None,
                    target_size: None,
                };
                let e = with_lnk(e, Some(rest), ansi);
                if !e.path.is_empty() {
                    entries.push(e);
                }
            }
            i += LNK_HEADER.len();
        } else {
            i += 1;
        }
    }
    Ok(JumpList {
        app_id: app_id.to_ascii_lowercase(),
        app_name: app_name(app_id),
        version: None,
        entries,
        notes: Vec::new(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn appid_lookup() {
        assert_eq!(app_name("9B9CDC69C1C24E2B").as_deref(), Some("Notepad 64-bit"));
        assert_eq!(app_name("1b4dd67f29cb1962").as_deref(), Some("Windows Explorer Pinned and Recent."));
        assert_eq!(app_id_from_file_name("5F7B5F1E01B83767.automaticDestinations-ms"), "5f7b5f1e01b83767");
        assert!(app_name("0000000000000000").is_none());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse_automatic(b"not a compound file", "x", encoding_rs::EUC_KR).is_err());
        assert!(parse_custom(&[0u8; 10], "x", encoding_rs::EUC_KR).is_err());
        assert!(parse_custom(&[0u8; 100], "x", encoding_rs::EUC_KR).unwrap().entries.is_empty());
    }
}
