//! 셸 항목(Shell Item) 해석 — 탐색기가 폴더 위치를 저장하는 형식.
//!
//! 셸백(ShellBags)과 바로 가기의 대상 목록(LinkTargetIDList)이 이 형식을 쓴다. 항목 하나가
//! 경로의 한 단계(내 PC → C:\ → Users → …)다. 모든 종류를 다 풀지 않고, 폴더 경로를 만드는 데
//! 필요한 종류만 읽는다. 모르는 종류는 안에 든 유니코드 문자열을 이름으로 쓴다(휴대폰 폴더 등).
//!
//! 근거: libyal/libfwsi 문서 "Windows Shell Item format" (Joachim Metz),
//!   확장 블록 0xBEEF0004의 위치 값은 Eric Zimmerman ExtensionBlocks 라이브러리(MIT)와 같다.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use st_core::bytes;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Root,
    Volume,
    Folder,
    File,
    Network,
    Uri,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellItem {
    pub kind: ItemKind,
    pub name: String,
    /// 확장 블록의 생성·접근 시각과 항목의 수정 시각 (FAT 형식, 2초 단위)
    pub created: Option<DateTime<Utc>>,
    pub accessed: Option<DateTime<Utc>>,
    pub modified: Option<DateTime<Utc>>,
}

/// 루트 폴더 GUID → 이름 (자주 나오는 것만).
fn known_folder(guid: &str) -> Option<&'static str> {
    Some(match guid {
        "20D04FE0-3AEA-1069-A2D8-08002B30309D" => "내 PC",
        "59031A47-3F72-44A7-89C5-5595FE6B30EE" => "사용자 폴더",
        "F02C1A0D-BE21-4350-88B0-7367FC96EF3C" => "네트워크",
        "208D2C60-3AEA-1069-A2D7-08002B30309D" => "네트워크",
        "031E4825-7B94-4DC3-B131-E946B44C8DD5" => "라이브러리",
        "645FF040-5081-101B-9F08-00AA002F954E" => "휴지통",
        "679F85CB-0220-4080-B29B-5540CC05AAB6" => "즐겨찾기(빠른 실행)",
        "21EC2020-3AEA-1069-A2DD-08002B30309D" => "제어판",
        "26EE0668-A00A-44D7-9371-BEB064C98683" => "제어판",
        "B4BFCC3A-DB2C-424C-B029-7FE99A87C641" => "바탕 화면",
        "FDD39AD0-238F-46AF-ADB4-6C85480369C7" => "문서",
        "374DE290-123F-4565-9164-39C4925E467B" => "다운로드",
        "018D5C66-4533-4307-9B53-224DE2ED1FE6" => "OneDrive",
        "F874310E-B6B7-47DC-BC84-B9E6B38F5903" => "홈",
        _ => return None,
    })
}

pub fn guid_at(d: &[u8], off: usize) -> Option<String> {
    let b = bytes::slice(d, off, 16)?;
    let d1 = bytes::u32_le(b, 0)?;
    let d2 = bytes::u16_le(b, 4)?;
    let d3 = bytes::u16_le(b, 6)?;
    let rest: Vec<String> = b.get(8..16)?.iter().map(|x| format!("{x:02X}")).collect();
    Some(format!("{d1:08X}-{d2:04X}-{d3:04X}-{}{}-{}", rest.first()?, rest.get(1)?, rest.get(2..8)?.concat()))
}

/// FAT 날짜·시각(날짜 2바이트 + 시각 2바이트). 값이 없거나 잘못되면 None.
fn fat_datetime(d: &[u8], off: usize) -> Option<DateTime<Utc>> {
    let date = bytes::u16_le(d, off)?;
    let time = bytes::u16_le(d, off + 2)?;
    if date == 0 {
        return None;
    }
    let y = 1980 + i32::from(date >> 9);
    let m = u32::from((date >> 5) & 0x0F);
    let day = u32::from(date & 0x1F);
    let h = u32::from(time >> 11);
    let min = u32::from((time >> 5) & 0x3F);
    let s = u32::from(time & 0x1F) * 2;
    NaiveDate::from_ymd_opt(y, m, day)?.and_hms_opt(h, min, s).map(|n| n.and_utc())
}

fn utf16_until_nul(d: &[u8], off: usize, max: usize) -> Option<String> {
    let s = bytes::utf16le_until_nul(d, off, max)?;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn ascii_until_nul(d: &[u8], off: usize, max: usize) -> Option<String> {
    let b = bytes::bytes_until_nul(d, off, max)?;
    if b.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(b).into_owned())
}

/// 확장 블록 0xBEEF0004 (긴 이름, 생성·접근 시각).
fn beef0004(item: &[u8]) -> Option<(String, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> {
    let sig = [0x04u8, 0x00, 0xEF, 0xBE];
    let pos = item.windows(4).position(|w| w == sig)?;
    let start = pos.checked_sub(4)?;
    let block = item.get(start..)?;
    let size = usize::from(bytes::u16_le(block, 0)?);
    let block = block.get(..size.min(block.len()))?;
    let version = bytes::u16_le(block, 2)?;
    let created = fat_datetime(block, 8);
    let accessed = fat_datetime(block, 12);
    let mut idx = 18usize;
    if version >= 7 {
        idx += 18;
    }
    if version >= 3 {
        idx += 2;
    }
    if version >= 9 {
        idx += 4;
    }
    if version >= 8 {
        idx += 4;
    }
    let name = utf16_until_nul(block, idx, 1024)?;
    Some((name, created, accessed))
}

/// 모르는 종류: 안에 든 첫 유니코드 문자열(2글자 이상, 표시 가능한 글자만)을 이름으로 쓴다.
fn guess_name(item: &[u8]) -> Option<String> {
    let mut i = 4usize;
    while i + 4 <= item.len() {
        if let Some(s) = bytes::utf16le_until_nul(item, i, 256) {
            let n = s.chars().count();
            if n >= 2 && s.chars().all(|c| !c.is_control() && c != '\u{FFFD}') && s.chars().any(char::is_alphanumeric) {
                return Some(s);
            }
        }
        i += 2;
    }
    None
}

/// 셸 항목 하나(앞의 크기 2바이트 포함)를 해석한다.
pub fn parse_item(item: &[u8]) -> Option<ShellItem> {
    let class = *item.get(2)?;
    let mut out = ShellItem { kind: ItemKind::Other, name: String::new(), created: None, accessed: None, modified: None };
    match class {
        0x1F => {
            out.kind = ItemKind::Root;
            let g = guid_at(item, 4)?;
            out.name = known_folder(&g).map(str::to_string).unwrap_or_else(|| format!("{{{g}}}"));
        }
        0x20..=0x2F if class != 0x2E => {
            out.kind = ItemKind::Volume;
            out.name = ascii_until_nul(item, 3, 20).unwrap_or_default().trim_end_matches('\\').to_string();
            if out.name.is_empty() {
                out.name = guess_name(item).unwrap_or_else(|| "드라이브".into());
            }
        }
        0x30..=0x3F => {
            out.kind = if class & 0x01 != 0 { ItemKind::Folder } else { ItemKind::File };
            out.modified = fat_datetime(item, 8);
            let unicode = class & 0x04 != 0;
            let short = if unicode { utf16_until_nul(item, 14, 260) } else { ascii_until_nul(item, 14, 260) };
            match beef0004(item) {
                Some((long, c, a)) => {
                    out.name = long;
                    out.created = c;
                    out.accessed = a;
                }
                None => out.name = short.unwrap_or_default(),
            }
        }
        0x40..=0x4F => {
            out.kind = ItemKind::Network;
            out.name = ascii_until_nul(item, 5, 260).unwrap_or_default();
        }
        0x61 => {
            out.kind = ItemKind::Uri;
            out.name = guess_name(item).unwrap_or_else(|| "URI".into());
        }
        0x74 => {
            // 사용자 파일 시스템 위임 항목: 안에 폴더 항목이 들어 있다.
            out.kind = ItemKind::Folder;
            match beef0004(item) {
                Some((long, c, a)) => {
                    out.name = long;
                    out.created = c;
                    out.accessed = a;
                }
                None => out.name = ascii_until_nul(item, 0x18, 260).unwrap_or_default(),
            }
        }
        _ => {
            out.name = beef0004(item).map(|b| b.0).or_else(|| guess_name(item)).unwrap_or_default();
        }
    }
    if out.name.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 항목 목록(IDList)을 차례로 해석한다. 크기가 0인 항목에서 끝난다.
pub fn parse_list(d: &[u8]) -> Vec<ShellItem> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some(size) = bytes::u16_le(d, pos) {
        let size = usize::from(size);
        if size < 3 {
            break;
        }
        let Some(item) = bytes::slice(d, pos, size) else { break };
        if let Some(i) = parse_item(item) {
            out.push(i);
        }
        pos += size;
        if out.len() > 128 {
            break;
        }
    }
    out
}

/// 항목들을 사람이 읽는 경로로 잇는다. 드라이브 다음에는 `\`, 루트 이름은 `내 PC`처럼 그대로 둔다.
pub fn join(items: &[&ShellItem]) -> String {
    let mut s = String::new();
    for it in items {
        if s.is_empty() {
            s.push_str(&it.name);
            continue;
        }
        if it.kind == ItemKind::Volume || it.kind == ItemKind::Network {
            // 드라이브·네트워크 경로는 그 자체로 시작점이다 (내 PC\C: 대신 C:)
            s = it.name.clone();
            continue;
        }
        if !s.ends_with('\\') {
            s.push('\\');
        }
        s.push_str(&it.name);
    }
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn root_and_volume_items() {
        // 내 PC 루트 항목: 크기 20, 종류 0x1F, 정렬 1바이트, GUID
        let mut root = vec![0x14, 0x00, 0x1F, 0x50];
        root.extend_from_slice(&[0xE0, 0x4F, 0xD0, 0x20, 0xEA, 0x3A, 0x69, 0x10, 0xA2, 0xD8, 0x08, 0x00, 0x2B, 0x30, 0x30, 0x9D]);
        let r = parse_item(&root).unwrap();
        assert_eq!(r.kind, ItemKind::Root);
        assert_eq!(r.name, "내 PC");

        let mut vol = vec![0x19, 0x00, 0x2F];
        vol.extend_from_slice(b"E:\\");
        vol.resize(0x19, 0);
        let v = parse_item(&vol).unwrap();
        assert_eq!(v.kind, ItemKind::Volume);
        assert_eq!(v.name, "E:");

        let mut list = root.clone();
        list.extend_from_slice(&vol);
        // 짧은 이름만 있는 폴더 항목 (ASCII)
        let mut folder = vec![0x00, 0x00, 0x31, 0x00, 0, 0, 0, 0, 0x21, 0x48, 0x00, 0x60, 0x10, 0x00];
        folder.extend_from_slice(b"REPORTS\0");
        let n = u16::try_from(folder.len()).unwrap();
        folder[0..2].copy_from_slice(&n.to_le_bytes());
        list.extend_from_slice(&folder);
        list.extend_from_slice(&[0, 0]);
        let items = parse_list(&list);
        assert_eq!(items.len(), 3);
        let refs: Vec<&ShellItem> = items.iter().collect();
        assert_eq!(join(&refs), "E:\\REPORTS");
        assert_eq!(items[2].modified.unwrap().to_rfc3339(), "2016-01-01T12:00:00+00:00");
    }

    #[test]
    fn garbage_never_panics() {
        for len in 0..64 {
            let d: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
            let _ = parse_list(&d);
            let _ = parse_item(&d);
        }
    }
}
