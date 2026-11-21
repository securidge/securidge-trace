//! 휴지통 `$I` 파일 파서.
//!
//! 파일을 휴지통에 버리면 `$Recycle.Bin\<SID>\` 아래에 두 파일이 생긴다.
//! `$R…`은 파일 내용, `$I…`는 원래 경로·크기·삭제 시각이다. 여기서는 `$I`만 읽는다.
//!
//! 형식 (리틀 엔디언)
//! ```text
//! 버전 1 (Vista ~ 8.1)                  버전 2 (Windows 10 이상)
//! 0x00  8  버전 = 1                     0x00  8  버전 = 2
//! 0x08  8  원래 파일 크기               0x08  8  원래 파일 크기
//! 0x10  8  삭제 시각 (FILETIME, UTC)    0x10  8  삭제 시각 (FILETIME, UTC)
//! 0x18  520 원래 경로 (UTF-16LE, 0 채움) 0x18  4  경로 글자 수 (종료 문자 포함)
//!                                       0x1C  n  원래 경로 (UTF-16LE)
//! ```
//!
//! 근거
//! - "Windows Recycle Bin $I file format" 문서 (gamingwithevets.github.io/blog/2023/01/15/rbin-docs.html)
//! - Champlain College Leahy Center, "Windows 10 Recycle Bin Activity"
//! - rifiuti2 매뉴얼 (삭제 시각은 UTC)
//!
//! 검증 상태: 형식 문서 기준 합성 데이터로만 테스트함. **실제 Windows에서 만든 $I 파일로는 미검증.**

use chrono::{DateTime, Utc};
use serde::Serialize;
use st_core::bytes;
use st_core::time::filetime_to_utc;

/// 버전 2에서 허용하는 경로 글자 수 상한 (NTFS 경로 최대 길이 32,767 + 종료 문자).
const MAX_PATH_UNITS: u32 = 32_768;
/// 버전 1의 경로 영역 크기 (UTF-16 260글자).
const V1_PATH_UNITS: usize = 260;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecycleEntry {
    pub version: u64,
    pub original_size: u64,
    pub deleted_at_raw: u64,
    pub deleted_at: Option<DateTime<Utc>>,
    pub original_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecycleError {
    /// 머리 부분(24바이트)도 없다.
    TooShort,
    UnknownVersion(u64),
    /// 버전 2의 경로 길이가 상한을 넘는다 (조작됐거나 손상된 파일).
    PathTooLong(u32),
    /// 적혀 있는 길이만큼 데이터가 없다.
    Truncated,
    /// 경로가 비어 있다.
    EmptyPath,
}

impl std::fmt::Display for RecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecycleError::TooShort => write!(f, "파일이 너무 짧음"),
            RecycleError::UnknownVersion(v) => write!(f, "알 수 없는 형식 버전 {v}"),
            RecycleError::PathTooLong(n) => write!(f, "경로 길이 값이 비정상적으로 큼 ({n})"),
            RecycleError::Truncated => write!(f, "파일이 중간에 잘림"),
            RecycleError::EmptyPath => write!(f, "원래 경로가 비어 있음"),
        }
    }
}

/// `$I` 파일 하나를 해석한다.
pub fn parse_index(data: &[u8]) -> Result<RecycleEntry, RecycleError> {
    let version = bytes::u64_le(data, 0x00).ok_or(RecycleError::TooShort)?;
    let original_size = bytes::u64_le(data, 0x08).ok_or(RecycleError::TooShort)?;
    let deleted_at_raw = bytes::u64_le(data, 0x10).ok_or(RecycleError::TooShort)?;

    let original_path = match version {
        1 => bytes::utf16le_until_nul(data, 0x18, V1_PATH_UNITS).ok_or(RecycleError::Truncated)?,
        2 => {
            let units = bytes::u32_le(data, 0x18).ok_or(RecycleError::Truncated)?;
            if units > MAX_PATH_UNITS {
                return Err(RecycleError::PathTooLong(units));
            }
            let units = usize::try_from(units).map_err(|_| RecycleError::Truncated)?;
            // 적혀 있는 길이만큼 실제로 데이터가 있는지 먼저 확인한다.
            let byte_len = units.checked_mul(2).ok_or(RecycleError::Truncated)?;
            if bytes::slice(data, 0x1C, byte_len).is_none() {
                return Err(RecycleError::Truncated);
            }
            bytes::utf16le_until_nul(data, 0x1C, units).ok_or(RecycleError::Truncated)?
        }
        other => return Err(RecycleError::UnknownVersion(other)),
    };

    if original_path.is_empty() {
        return Err(RecycleError::EmptyPath);
    }

    Ok(RecycleEntry {
        version,
        original_size,
        deleted_at_raw,
        deleted_at: filetime_to_utc(deleted_at_raw),
        original_path,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    /// 2021-02-08T12:41:58Z
    const FT: u64 = (1_612_788_118 + 11_644_473_600) * 10_000_000;

    fn make_v2(path: &str) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&2u64.to_le_bytes());
        d.extend_from_slice(&1234u64.to_le_bytes());
        d.extend_from_slice(&FT.to_le_bytes());
        let units = path.encode_utf16().count() as u32 + 1;
        d.extend_from_slice(&units.to_le_bytes());
        d.extend_from_slice(&utf16(path));
        d.extend_from_slice(&[0, 0]);
        d
    }

    fn make_v1(path: &str) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&1u64.to_le_bytes());
        d.extend_from_slice(&99u64.to_le_bytes());
        d.extend_from_slice(&FT.to_le_bytes());
        let mut p = utf16(path);
        p.resize(520, 0);
        d.extend_from_slice(&p);
        d
    }

    #[test]
    fn parses_version_2_with_korean_path() {
        let e = parse_index(&make_v2("C:\\Users\\kim\\Desktop\\견적서 최종.xlsx")).unwrap();
        assert_eq!(e.version, 2);
        assert_eq!(e.original_size, 1234);
        assert_eq!(e.original_path, "C:\\Users\\kim\\Desktop\\견적서 최종.xlsx");
        assert_eq!(e.deleted_at.unwrap().to_rfc3339(), "2021-02-08T12:41:58+00:00");
    }

    #[test]
    fn parses_version_1() {
        let d = make_v1("C:\\temp\\a.txt");
        assert_eq!(d.len(), 544);
        let e = parse_index(&d).unwrap();
        assert_eq!(e.version, 1);
        assert_eq!(e.original_path, "C:\\temp\\a.txt");
    }

    #[test]
    fn rejects_bad_input_without_panicking() {
        assert_eq!(parse_index(&[]), Err(RecycleError::TooShort));
        assert_eq!(parse_index(&[0u8; 23]), Err(RecycleError::TooShort));
        let mut d = make_v2("C:\\a");
        d[0] = 7;
        assert_eq!(parse_index(&d), Err(RecycleError::UnknownVersion(7)));

        // 조작된 길이 값: 상한 초과
        let mut d = make_v2("C:\\a");
        d[0x18..0x1C].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_index(&d), Err(RecycleError::PathTooLong(u32::MAX)));

        // 길이 값은 정상 범위지만 데이터가 모자람
        let mut d = make_v2("C:\\a");
        d[0x18..0x1C].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(parse_index(&d), Err(RecycleError::Truncated));
    }

    #[test]
    fn zero_deletion_time_is_none() {
        let mut d = make_v2("C:\\a");
        d[0x10..0x18].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(parse_index(&d).unwrap().deleted_at, None);
    }

    /// 간이 내성 시험: 모든 길이로 자르고, 모든 바이트를 바꿔 봐도 패닉하지 않아야 한다.
    /// (cargo-fuzz를 대신하지는 못한다. 정식 퍼징은 별도 과제.)
    #[test]
    fn survives_truncation_and_byte_flips() {
        for sample in [make_v1("C:\\temp\\a.txt"), make_v2("C:\\Users\\kim\\문서.hwp")] {
            for len in 0..sample.len() {
                let _ = parse_index(&sample[..len]);
            }
            for i in 0..sample.len() {
                for v in [0x00u8, 0xFF, 0x80] {
                    let mut m = sample.clone();
                    m[i] = v;
                    let _ = parse_index(&m);
                }
            }
        }
    }
}
