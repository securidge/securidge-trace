//! Windows 이벤트 로그(.evtx)에서 필요한 사건만 뽑는다.
//!
//! 해석은 evtx crate(omerbenamram, MIT/Apache-2.0)가 하고, 여기서는 사건 ID별로 필요한 값만 고른다.
//!
//! | 로그 | 사건 ID | 뜻 |
//! |---|---|---|
//! | Security | 1102 | 보안 로그가 지워짐 |
//! | System | 104 | 로그가 지워짐 (어느 로그인지 함께 기록) |
//! | Security | 4616 | 시스템 시각이 바뀜 (PreviousTime, NewTime) |
//! | System (Kernel-General) | 1 | 시스템 시각이 바뀜 (OldTime, NewTime) |
//! | Microsoft-Windows-Partition/Diagnostic | 1006 | 저장장치 연결·분리. 제조사·모델·일련번호와 볼륨 부트 레코드(Vbr0) |
//! | Microsoft-Windows-WLAN-AutoConfig/Operational | 8001 / 8003 | 무선 네트워크 연결 / 연결 끊김 |
//!
//! 근거: Microsoft Learn 보안 감사 사건 문서(1102, 4616), forensics.wiki "USB History Viewing"(1006),
//! SANS "Making the most out of WLAN event log artifacts"(8001, 8003).
//!
//! 볼륨 시리얼 번호(VSN)는 Vbr0의 파일 시스템별 위치에서 꺼낸다:
//! NTFS 0x48(하위 4바이트), FAT32 0x43, FAT12/16 0x27, exFAT 0x64.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use evtx::{EvtxParser, ParserSettings};
use serde::Serialize;
use serde_json::Value;

/// 파일 하나에서 읽는 최대 레코드 수.
const MAX_RECORDS: usize = 2_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub record_id: u64,
    pub event_id: u32,
    pub time: Option<DateTime<Utc>>,
    pub channel: String,
    pub provider: String,
    /// 사건별로 고른 값
    pub data: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LogRead {
    pub events: Vec<LogEvent>,
    /// 파일 안 가장 오래된·최근 레코드 시각 (데이터 범위 표시용)
    pub earliest: Option<DateTime<Utc>>,
    pub latest: Option<DateTime<Utc>>,
    pub total_records: usize,
    pub bad_records: usize,
}

/// 관심 있는 (채널, 사건 ID)인가.
fn wanted(channel: &str, event_id: u32) -> bool {
    let c = channel.to_ascii_lowercase();
    match event_id {
        1102 | 4616 => c == "security",
        104 | 1 => c == "system",
        1006 => c.contains("partition/diagnostic"),
        8001 | 8003 => c.contains("wlan-autoconfig"),
        _ => false,
    }
}

fn s(v: &Value) -> Option<String> {
    match v {
        Value::String(x) => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// EventData / UserData 아래 값들을 평평하게 모은다.
fn flatten(v: &Value, prefix: &str, out: &mut BTreeMap<String, String>, depth: usize) {
    if depth > 4 {
        return;
    }
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k.starts_with("#attributes") || k == "xmlns" {
                    continue;
                }
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(val, &key, out, depth + 1);
            }
        }
        other => {
            if let Some(x) = s(other) {
                out.insert(prefix.to_string(), x);
            }
        }
    }
}

/// JSON 경로를 따라 값을 꺼낸다 (없으면 Null).
fn at<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    static NULL: Value = Value::Null;
    let mut cur = v;
    for k in path {
        match cur.get(k) {
            Some(x) => cur = x,
            None => return &NULL,
        }
    }
    cur
}

fn parse_time(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let t = v?.as_str()?;
    DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&Utc))
}

/// .evtx 파일 하나를 읽는다.
pub fn read(path: &Path) -> Result<LogRead, String> {
    let settings = ParserSettings::default().num_threads(1);
    let mut parser = EvtxParser::from_path(path)
        .map_err(|e| format!("이벤트 로그를 열지 못함: {e}"))?
        .with_configuration(settings);
    let mut out = LogRead::default();
    for rec in parser.records_json_value() {
        if out.total_records >= MAX_RECORDS {
            break;
        }
        out.total_records += 1;
        let rec = match rec {
            Ok(r) => r,
            Err(_) => {
                out.bad_records += 1;
                continue;
            }
        };
        let ev = at(&rec.data, &["Event"]);
        let sys = at(ev, &["System"]);
        let time = parse_time(at(sys, &["TimeCreated", "#attributes", "SystemTime"]).as_ref_opt());
        if let Some(t) = time {
            out.earliest = Some(out.earliest.map_or(t, |e| e.min(t)));
            out.latest = Some(out.latest.map_or(t, |l| l.max(t)));
        }
        let event_id = match at(sys, &["EventID"]) {
            Value::Number(n) => n.as_u64(),
            Value::Object(o) => o.get("#text").and_then(Value::as_u64),
            Value::String(x) => x.parse().ok(),
            _ => None,
        }
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
        let channel = at(sys, &["Channel"]).as_str().unwrap_or_default().to_string();
        if !wanted(&channel, event_id) {
            continue;
        }
        let provider =
            at(sys, &["Provider", "#attributes", "Name"]).as_str().unwrap_or_default().to_string();
        // Kernel-General 1번만 시각 변경이다 (System 로그의 1번은 다른 공급자도 쓴다).
        if event_id == 1 && provider != "Microsoft-Windows-Kernel-General" {
            continue;
        }
        let mut data = BTreeMap::new();
        flatten(at(ev, &["EventData"]), "", &mut data, 0);
        flatten(at(ev, &["UserData"]), "", &mut data, 0);
        if event_id == 1006 {
            if let Some(vsn) = vsn_from_event(&data) {
                data.insert("VolumeSerial".to_string(), vsn);
            }
            // 볼륨 부트 레코드 등 큰 16진수 값은 근거 파일을 불리지 않도록 뺀다.
            data.retain(|k, v| !(k.starts_with("Vbr") || k.starts_with("Mbr") || k.starts_with("Gpt") || k.starts_with("PartitionTable")) || v.len() < 64);
        }
        out.events.push(LogEvent {
            record_id: rec.event_record_id,
            event_id,
            time,
            channel,
            provider,
            data,
        });
    }
    Ok(out)
}

trait AsRefOpt {
    fn as_ref_opt(&self) -> Option<&Value>;
}
impl AsRefOpt for Value {
    fn as_ref_opt(&self) -> Option<&Value> {
        if self.is_null() {
            None
        } else {
            Some(self)
        }
    }
}

fn hex_bytes(s: &str, max: usize) -> Vec<u8> {
    let clean: Vec<u8> = s.bytes().filter(u8::is_ascii_hexdigit).collect();
    clean
        .chunks_exact(2)
        .take(max)
        .filter_map(|c| std::str::from_utf8(c).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

/// 볼륨 부트 레코드에서 볼륨 시리얼 번호(`1A2B-3C4D`)를 꺼낸다.
pub fn vsn_from_vbr(vbr: &[u8]) -> Option<String> {
    let oem = st_core::bytes::slice(vbr, 3, 8)?;
    let off = if oem.starts_with(b"NTFS") {
        0x48
    } else if oem.starts_with(b"EXFAT") {
        0x64
    } else if st_core::bytes::slice(vbr, 0x52, 5) == Some(b"FAT32") {
        0x43
    } else if st_core::bytes::slice(vbr, 0x36, 3) == Some(b"FAT") {
        0x27
    } else {
        return None;
    };
    let v = st_core::bytes::u32_le(vbr, off)?;
    Some(format!("{:04X}-{:04X}", v >> 16, v & 0xFFFF))
}

fn vsn_from_event(data: &BTreeMap<String, String>) -> Option<String> {
    for key in ["Vbr0", "Vbr1", "Vbr2", "Vbr3"] {
        if let Some(h) = data.get(key) {
            if let Some(v) = vsn_from_vbr(&hex_bytes(h, 512)) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn vsn_from_ntfs_and_fat32() {
        let mut ntfs = vec![0u8; 512];
        ntfs[3..11].copy_from_slice(b"NTFS    ");
        ntfs[0x48..0x50].copy_from_slice(&0x1122_3344_1A2B_3C4Du64.to_le_bytes());
        assert_eq!(vsn_from_vbr(&ntfs).as_deref(), Some("1A2B-3C4D"));

        let mut fat = vec![0u8; 512];
        fat[3..11].copy_from_slice(b"MSDOS5.0");
        fat[0x52..0x57].copy_from_slice(b"FAT32");
        fat[0x43..0x47].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert_eq!(vsn_from_vbr(&fat).as_deref(), Some("DEAD-BEEF"));

        let mut exfat = vec![0u8; 512];
        exfat[3..11].copy_from_slice(b"EXFAT   ");
        exfat[0x64..0x68].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        assert_eq!(vsn_from_vbr(&exfat).as_deref(), Some("0102-0304"));

        assert_eq!(vsn_from_vbr(&[0u8; 10]), None);
        assert_eq!(vsn_from_vbr(&[]), None);
    }

    #[test]
    fn hex_parsing_is_lenient() {
        assert_eq!(hex_bytes("4E54 46zz53", 10), vec![0x4E, 0x54, 0x46, 0x53]);
    }
}
