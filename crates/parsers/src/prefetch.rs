//! Prefetch(.pf) 파서 — "어떤 프로그램을 언제, 몇 번 실행했는가".
//!
//! Windows는 프로그램을 실행하면 `C:\Windows\Prefetch\<실행 파일 이름>-<해시>.pf`를 만들고,
//! 실행할 때마다 실행 횟수와 최근 실행 시각(최대 8개)을 고친다. 파일 안에는 그 프로그램이
//! 실행되며 읽은 파일 목록과 **볼륨(드라이브)의 일련번호**도 있어, 프로그램이 USB에서
//! 실행됐는지 확인하는 근거가 된다.
//!
//! 한계
//! - Windows Server는 기본 설정에서 Prefetch를 만들지 않는다.
//! - SSD에서도 기본으로 켜져 있지만, 관리자가 끌 수 있다. 파일이 없으면 "실행 안 함"이 아니라 "기록 없음"이다.
//! - 오래된 항목은 Windows가 지운다 (최대 1,024개).
//!
//! 근거: libscca 문서 "Windows Prefetch File (PF) format" (Joachim Metz),
//!   Eric Zimmerman의 Prefetch 라이브러리(MIT)의 형식별 위치 값.
//! 지원 형식: 23(Windows 7), 26(Windows 8.x), 30·31(Windows 10·11, MAM 압축).
//! 검증: tests/prefetch_samples.rs — 공개 표본 파일의 실행 횟수·시각·볼륨 값을 비교.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use st_core::bytes;
use st_core::time::filetime_to_utc;

use crate::xpress;

const SCCA: u32 = 0x4143_4353;
const HEADER_SIZE: usize = 84;
/// 파일 이름 목록 상한
const MAX_FILES: usize = 4_096;
const MAX_VOLUMES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PfVolume {
    pub device_path: String,
    /// Windows가 보여주는 형태 (`1A2B-3C4D`)
    pub serial: String,
    pub created: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefetchInfo {
    pub version: u32,
    pub executable: String,
    pub hash: String,
    pub run_count: u32,
    /// 최근 실행 시각 (최신이 먼저)
    pub last_runs: Vec<DateTime<Utc>>,
    pub last_runs_raw: Vec<u64>,
    pub volumes: Vec<PfVolume>,
    pub files: Vec<String>,
    /// 파일 목록에서 찾은 실행 파일 자체의 경로 (`\VOLUME{…}\PROGRAM FILES\…\APP.EXE`)
    pub executable_path: Option<String>,
}

impl PrefetchInfo {
    /// 실행 파일이 있던 볼륨의 일련번호.
    pub fn executable_volume(&self) -> Option<&PfVolume> {
        let p = self.executable_path.as_deref()?.to_ascii_uppercase();
        self.volumes.iter().find(|v| {
            let d = v.device_path.to_ascii_uppercase();
            !d.is_empty() && p.starts_with(&d)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefetchError {
    NotPrefetch,
    Unsupported(u32),
    Damaged(String),
}

impl std::fmt::Display for PrefetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrefetchError::NotPrefetch => write!(f, "Prefetch 형식이 아님"),
            PrefetchError::Unsupported(v) => write!(f, "지원하지 않는 Prefetch 형식 {v}"),
            PrefetchError::Damaged(m) => write!(f, "Prefetch 파일이 손상됨 ({m})"),
        }
    }
}

struct Layout {
    info_size: usize,
    last_runs: usize,
    run_count: usize,
    run_count_may_move: bool,
    volume_entry: usize,
}

fn layout(version: u32) -> Option<Layout> {
    Some(match version {
        23 => Layout { info_size: 156, last_runs: 1, run_count: 68, run_count_may_move: false, volume_entry: 104 },
        26 => Layout { info_size: 224, last_runs: 8, run_count: 124, run_count_may_move: false, volume_entry: 104 },
        30 | 31 => Layout { info_size: 224, last_runs: 8, run_count: 124, run_count_may_move: true, volume_entry: 96 },
        _ => return None,
    })
}

/// MAM 압축(Windows 10 이후)이면 풀고, 아니면 그대로 돌려준다.
pub fn unpack(data: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, PrefetchError> {
    if bytes::slice(data, 0, 3) == Some(b"MAM") {
        let flag = *data.get(3).ok_or(PrefetchError::NotPrefetch)?;
        if flag & 0x0F != 4 {
            return Err(PrefetchError::Damaged(format!("압축 방식 {}", flag & 0x0F)));
        }
        let size = bytes::u32_le(data, 4).ok_or(PrefetchError::NotPrefetch)?;
        let start = if flag & 0x80 != 0 { 12 } else { 8 };
        let body = data.get(start..).ok_or(PrefetchError::NotPrefetch)?;
        let size = usize::try_from(size).map_err(|_| PrefetchError::Damaged("크기".into()))?;
        let out = xpress::decompress(body, size).map_err(|e| PrefetchError::Damaged(e.to_string()))?;
        return Ok(std::borrow::Cow::Owned(out));
    }
    Ok(std::borrow::Cow::Borrowed(data))
}

fn i32_at(d: &[u8], off: usize) -> Option<usize> {
    usize::try_from(bytes::u32_le(d, off)?).ok()
}

/// .pf 파일 하나를 해석한다.
pub fn parse(raw: &[u8]) -> Result<PrefetchInfo, PrefetchError> {
    let data = unpack(raw)?;
    let d: &[u8] = &data;
    if bytes::u32_le(d, 4) != Some(SCCA) {
        return Err(PrefetchError::NotPrefetch);
    }
    let version = bytes::u32_le(d, 0).ok_or(PrefetchError::NotPrefetch)?;
    let lay = layout(version).ok_or(PrefetchError::Unsupported(version))?;
    let dmg = |m: &str| PrefetchError::Damaged(m.to_string());

    let executable = bytes::utf16le_until_nul(d, 16, 30).ok_or_else(|| dmg("이름"))?.trim().to_string();
    let hash = format!("{:08X}", bytes::u32_le(d, 76).ok_or_else(|| dmg("해시"))?);

    let info = bytes::slice(d, HEADER_SIZE, lay.info_size).ok_or_else(|| dmg("파일 정보"))?;
    let names_off = i32_at(info, 16).ok_or_else(|| dmg("파일 정보"))?;
    let names_size = i32_at(info, 20).ok_or_else(|| dmg("파일 정보"))?;
    let vol_off = i32_at(info, 24).ok_or_else(|| dmg("파일 정보"))?;
    let vol_count = i32_at(info, 28).ok_or_else(|| dmg("파일 정보"))?;
    let vol_size = i32_at(info, 32).ok_or_else(|| dmg("파일 정보"))?;

    let mut last_runs = Vec::new();
    let mut last_runs_raw = Vec::new();
    for i in 0..lay.last_runs {
        let raw_t = bytes::u64_le(info, 44 + i * 8).ok_or_else(|| dmg("실행 시각"))?;
        if raw_t == 0 {
            continue;
        }
        if let Some(t) = filetime_to_utc(raw_t) {
            last_runs.push(t);
            last_runs_raw.push(raw_t);
        }
    }
    let mut run_count = bytes::u32_le(info, lay.run_count).ok_or_else(|| dmg("실행 횟수"))?;
    // 최근 Windows 10/11은 실행 횟수 위치가 8바이트 앞으로 옮겨졌다. 옛 위치 4바이트 앞이 0이 아니면 새 위치다.
    if lay.run_count_may_move && bytes::u32_le(info, lay.run_count - 4).is_some_and(|v| v != 0) {
        run_count = bytes::u32_le(info, lay.run_count - 8).ok_or_else(|| dmg("실행 횟수"))?;
    }

    // 파일 이름 목록 (UTF-16, NUL로 구분)
    let mut files = Vec::new();
    if let Some(block) = bytes::slice(d, names_off, names_size) {
        let units: Vec<u16> = block.chunks_exact(2).map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)])).collect();
        for part in units.split(|&u| u == 0) {
            if part.is_empty() {
                continue;
            }
            files.push(String::from_utf16_lossy(part));
            if files.len() >= MAX_FILES {
                break;
            }
        }
    }

    // 볼륨 정보
    let mut volumes = Vec::new();
    if let Some(vols) = bytes::slice(d, vol_off, vol_size) {
        for j in 0..vol_count.min(MAX_VOLUMES) {
            let Some(entry) = bytes::slice(vols, j * lay.volume_entry, lay.volume_entry) else { break };
            let (Some(dev_off), Some(dev_chars)) = (i32_at(entry, 0), i32_at(entry, 4)) else { break };
            let created_raw = bytes::u64_le(entry, 8).unwrap_or(0);
            let serial = bytes::u32_le(entry, 16).unwrap_or(0);
            let device_path = bytes::slice(vols, dev_off, dev_chars.saturating_mul(2))
                .map(|b| {
                    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)])).collect();
                    String::from_utf16_lossy(&u)
                })
                .unwrap_or_default();
            volumes.push(PfVolume {
                device_path,
                serial: format!("{:04X}-{:04X}", serial >> 16, serial & 0xFFFF),
                created: filetime_to_utc(created_raw),
            });
        }
    }

    let exe_upper = executable.to_ascii_uppercase();
    let executable_path = if exe_upper.is_empty() {
        None
    } else {
        files
            .iter()
            .find(|f| f.rsplit('\\').next().is_some_and(|base| base.to_ascii_uppercase().starts_with(&exe_upper)))
            .cloned()
    };

    // 최신 실행이 먼저 오도록 (Windows가 이미 그렇게 저장하지만 한 번 더 정렬)
    let mut pairs: Vec<(DateTime<Utc>, u64)> = last_runs.into_iter().zip(last_runs_raw).collect();
    pairs.sort_by(|a, b| b.0.cmp(&a.0));
    let (last_runs, last_runs_raw) = pairs.into_iter().unzip();

    Ok(PrefetchInfo { version, executable, hash, run_count, last_runs, last_runs_raw, volumes, files, executable_path })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_prefetch() {
        assert_eq!(parse(b"hello world, not a prefetch file at all"), Err(PrefetchError::NotPrefetch));
        assert!(parse(b"MAM\x04\x10\x00\x00\x00garbage").is_err());
        assert!(parse(&[]).is_err());
    }
}
