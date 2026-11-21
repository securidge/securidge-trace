//! 실행·열람 흔적을 담은 레지스트리 값들.
//!
//! | 흔적 | 하이브 | 키 | 알려주는 것 |
//! |---|---|---|---|
//! | UserAssist | 사용자 NTUSER.DAT | `Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{GUID}\Count` | 탐색기·시작 메뉴로 실행한 프로그램, 횟수, 마지막 실행 |
//! | ShellBags | 사용자 UsrClass.dat, NTUSER.DAT | `…\Shell\BagMRU` | 탐색기로 열어 본 폴더 (USB·휴대폰·네트워크 폴더 포함) |
//! | BAM | SYSTEM | `Services\bam\State\UserSettings\<SID>` | 사용자별 프로그램 마지막 실행 시각 (Windows 10 1709 이후) |
//! | Amcache | Amcache.hve | `Root\InventoryApplicationFile` | PC에 있었던 실행 파일의 경로·SHA-1·처음 기록된 무렵 |
//!
//! 근거
//! - UserAssist: 값 이름은 ROT13, 값(72바이트)의 4번째 바이트부터 실행 횟수, 60번째부터 마지막 실행
//!   FILETIME. (Didier Stevens "UserAssist" 연구, SANS DFIR 포스터)
//! - ShellBags: BagMRU 아래 번호 키마다 값 하나가 셸 항목이다. 부모 키의 MRUListEx 첫 번째 항목이
//!   가장 최근에 다룬 폴더이고, 그때 부모 키의 마지막 기록 시각이 바뀐다. (SANS "Shellbags" 연구, libfwsi)
//! - BAM: 값 이름이 실행 파일의 장치 경로(`\Device\HarddiskVolume3\…`), 값 앞 8바이트가 FILETIME.
//! - Amcache: Windows 10 1709 이후 형식. `FileId`는 앞 4자리 `0000` + SHA-1.
//!
//! 한계 (보고서에 그대로 적는다)
//! - UserAssist는 탐색기를 거친 실행만 남는다. 명령 창에서 실행한 것은 남지 않는다.
//! - ShellBags의 시각은 "그 폴더를 마지막으로 다룬 무렵"이며, 가장 최근 항목에만 붙일 수 있다.
//! - Amcache의 키 시각은 실행 시각이 아니다. "이 파일이 PC에 있었다"는 근거로만 쓴다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use st_core::bytes;
use st_core::time::filetime_to_utc;

use crate::hive::{Hive, Key};
use crate::shellitem::{self, ShellItem};

const MAX_SHELLBAGS: usize = 20_000;
const MAX_DEPTH: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAssistEntry {
    pub path: String,
    pub run_count: u32,
    pub focus_seconds: u32,
    pub last_run: Option<DateTime<Utc>>,
    pub last_run_raw: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellBag {
    pub path: String,
    /// BagMRU 안의 위치 (예: `BagMRU\0\2\1`)
    pub key_path: String,
    /// 이 폴더를 마지막으로 다룬 무렵 (부모 키의 MRU 첫 항목일 때만)
    pub last_interacted: Option<DateTime<Utc>>,
    /// 폴더의 생성·접근·수정 시각 (셸 항목 안의 값, 2초 단위)
    pub created: Option<DateTime<Utc>>,
    pub accessed: Option<DateTime<Utc>>,
    pub modified: Option<DateTime<Utc>>,
    /// 루트 종류 (드라이브, 네트워크, 휴대폰 등 판단용)
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BamEntry {
    pub sid: String,
    /// 장치 경로 그대로 (`\Device\HarddiskVolume3\Windows\…`)
    pub path: String,
    pub last_run: Option<DateTime<Utc>>,
    pub last_run_raw: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmcacheFile {
    pub path: String,
    pub sha1: Option<String>,
    pub size: Option<String>,
    pub product: Option<String>,
    pub publisher: Option<String>,
    pub link_date: Option<String>,
    /// 키의 마지막 기록 시각 (처음 기록된 무렵으로 본다)
    pub key_time: DateTime<Utc>,
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            _ => c,
        })
        .collect()
}

/// 알려진 폴더 GUID로 시작하는 경로를 사람이 읽는 경로로 바꾼다.
pub fn expand_known_folder(path: &str) -> String {
    const MAP: [(&str, &str); 12] = [
        ("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}", "C:\\Windows\\System32"),
        ("{6D809377-6AF0-444B-8957-A3773F02200E}", "C:\\Program Files"),
        ("{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}", "C:\\Program Files (x86)"),
        ("{F38BF404-1D43-42F2-9305-67DE0B28FC23}", "C:\\Windows"),
        ("{D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27}", "C:\\Windows\\SysWOW64"),
        ("{374DE290-123F-4565-9164-39C4925E467B}", "%사용자%\\Downloads"),
        ("{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}", "%사용자%\\Desktop"),
        ("{FDD39AD0-238F-46AF-ADB4-6C85480369C7}", "%사용자%\\Documents"),
        ("{0139D44E-6AFE-49F2-8690-3DAFCAE6FFB8}", "시작 메뉴\\Programs"),
        ("{A77F5D77-2E2B-44C3-A6A2-ABA601054A51}", "시작 메뉴(사용자)\\Programs"),
        ("{9E3995AB-1F9C-4F13-B827-48B24B6C7174}", "작업 표시줄 고정"),
        ("{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}", "%사용자%\\AppData\\Local"),
    ];
    for (g, name) in MAP {
        if path.len() >= g.len() && path.get(..g.len()).is_some_and(|p| p.eq_ignore_ascii_case(g)) {
            return format!("{name}{}", path.get(g.len()..).unwrap_or(""));
        }
    }
    path.to_string()
}

/// NTUSER.DAT의 UserAssist.
pub fn user_assist(ntuser: &mut Hive) -> Vec<UserAssistEntry> {
    let mut out = Vec::new();
    let Some(mut root) = ntuser.key("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\UserAssist") else {
        return out;
    };
    for mut guid in ntuser.subkeys(&mut root) {
        let Some(count) = ntuser.subkey(&mut guid, "Count") else { continue };
        for (name, data) in count.binary_values() {
            let decoded = rot13(&name);
            if decoded.starts_with("UEME_") || data.len() < 68 {
                continue;
            }
            let raw = bytes::u64_le(&data, 60).unwrap_or(0);
            out.push(UserAssistEntry {
                path: expand_known_folder(&decoded),
                run_count: bytes::u32_le(&data, 4).unwrap_or(0),
                focus_seconds: bytes::u32_le(&data, 12).unwrap_or(0) / 1000,
                last_run: filetime_to_utc(raw),
                last_run_raw: raw,
            });
        }
    }
    out.sort_by(|a, b| b.last_run.cmp(&a.last_run).then(a.path.cmp(&b.path)));
    out
}

/// MRUListEx 값의 첫 번호 (가장 최근 항목). -1(0xFFFFFFFF)은 끝 표시.
fn mru_first(key: &Key) -> Option<String> {
    let b = key.binary("MRUListEx")?;
    let n = bytes::u32_le(&b, 0)?;
    if n == u32::MAX {
        None
    } else {
        Some(n.to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    hive: &mut Hive,
    key: &mut Key,
    key_path: &str,
    prefix: &mut Vec<ShellItem>,
    depth: usize,
    out: &mut Vec<ShellBag>,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_SHELLBAGS {
        return;
    }
    let first = mru_first(key);
    let parent_time = key.last_written();
    let values = key.binary_values();
    for mut child in hive.subkeys(key) {
        if out.len() >= MAX_SHELLBAGS {
            return;
        }
        let name = child.name().to_string();
        if name.parse::<u32>().is_err() {
            continue;
        }
        let Some((_, data)) = values.iter().find(|(n, _)| *n == name) else { continue };
        let Some(item) = shellitem::parse_list(data).into_iter().next() else { continue };
        prefix.push(item);
        let refs: Vec<&ShellItem> = prefix.iter().collect();
        let path = shellitem::join(&refs);
        let last = prefix.last();
        let child_path = format!("{key_path}\\{name}");
        out.push(ShellBag {
            path,
            key_path: child_path.clone(),
            last_interacted: if first.as_deref() == Some(name.as_str()) { Some(parent_time) } else { None },
            created: last.and_then(|i| i.created),
            accessed: last.and_then(|i| i.accessed),
            modified: last.and_then(|i| i.modified),
            root: prefix.first().map(|i| format!("{:?}", i.kind)).unwrap_or_default(),
        });
        walk(hive, &mut child, &child_path, prefix, depth + 1, out);
        prefix.pop();
    }
}

/// UsrClass.dat 또는 NTUSER.DAT의 ShellBags.
pub fn shellbags(hive: &mut Hive) -> Vec<ShellBag> {
    let mut out = Vec::new();
    for path in [
        "Local Settings\\Software\\Microsoft\\Windows\\Shell\\BagMRU",
        "Software\\Microsoft\\Windows\\Shell\\BagMRU",
        "Software\\Microsoft\\Windows\\ShellNoRoam\\BagMRU",
    ] {
        if let Some(mut k) = hive.key(path) {
            let mut prefix = Vec::new();
            walk(hive, &mut k, "BagMRU", &mut prefix, 0, &mut out);
        }
    }
    out
}

/// SYSTEM 하이브의 BAM.
pub fn bam(system: &mut Hive) -> Vec<BamEntry> {
    let ccs = system.current_control_set();
    let mut out = Vec::new();
    for base in [format!("{ccs}\\Services\\bam\\State\\UserSettings"), format!("{ccs}\\Services\\bam\\UserSettings")] {
        let Some(mut root) = system.key(&base) else { continue };
        for user in system.subkeys(&mut root) {
            let sid = user.name().to_string();
            for (name, data) in user.binary_values() {
                if data.len() < 8 || !name.starts_with('\\') {
                    continue;
                }
                let raw = bytes::u64_le(&data, 0).unwrap_or(0);
                out.push(BamEntry { sid: sid.clone(), path: name, last_run: filetime_to_utc(raw), last_run_raw: raw });
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out.sort_by(|a, b| b.last_run.cmp(&a.last_run).then(a.path.cmp(&b.path)));
    out
}

/// Amcache.hve의 실행 파일 목록.
pub fn amcache(hive: &mut Hive) -> Vec<AmcacheFile> {
    let mut out = Vec::new();
    let Some(mut root) = hive.key("Root\\InventoryApplicationFile") else { return out };
    for k in hive.subkeys(&mut root).into_iter().take(200_000) {
        let Some(path) = k.text("LowerCaseLongPath").filter(|p| !p.is_empty()) else { continue };
        let sha1 = k.text("FileId").map(|f| {
            let f = f.trim().to_ascii_lowercase();
            if f.len() == 44 && f.starts_with("0000") {
                f.get(4..).unwrap_or("").to_string()
            } else {
                f
            }
        });
        out.push(AmcacheFile {
            path,
            sha1: sha1.filter(|s| !s.is_empty()),
            size: k.text("Size"),
            product: k.text("ProductName").filter(|s| !s.is_empty()),
            publisher: k.text("Publisher").filter(|s| !s.is_empty()),
            link_date: k.text("LinkDate").filter(|s| !s.is_empty()),
            key_time: k.last_written(),
        });
    }
    out.sort_by(|a, b| b.key_time.cmp(&a.key_time).then(a.path.cmp(&b.path)));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rot13_and_known_folders() {
        assert_eq!(rot13("{6Q809377-6NS0-444O-8957-N3773S02200R}\\Abgrcnq++\\abgrcnq++.rkr"), "{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe");
        assert_eq!(
            expand_known_folder("{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe"),
            "C:\\Program Files\\Notepad++\\notepad++.exe"
        );
        assert_eq!(expand_known_folder("C:\\tools\\a.exe"), "C:\\tools\\a.exe");
    }
}
