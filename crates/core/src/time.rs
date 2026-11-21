//! Windows·브라우저 시각 값을 UTC로 바꾸는 함수.
//!
//! 근거
//! - FILETIME: 1601-01-01 00:00:00 UTC부터 100나노초 단위로 센 64비트 값.
//!   Microsoft Learn "FILETIME structure (minwinbase.h)"
//! - Chromium(WebKit) 시각: 1601-01-01 UTC부터 마이크로초 단위.
//!   Chromium 소스 base/time/time.h ("Time ... microseconds since the Windows epoch")
//! - Firefox PRTime: 1970-01-01 UTC부터 마이크로초 단위. (NSPR prtime.h)

use chrono::{DateTime, Utc};

/// 1601-01-01과 1970-01-01 사이의 초.
const WINDOWS_TO_UNIX_EPOCH_SECS: i64 = 11_644_473_600;

/// FILETIME(100ns, 1601 기준) → UTC. 0은 "값 없음"으로 본다.
pub fn filetime_to_utc(filetime: u64) -> Option<DateTime<Utc>> {
    if filetime == 0 {
        return None;
    }
    let secs_since_1601 = i64::try_from(filetime / 10_000_000).ok()?;
    let nanos = u32::try_from((filetime % 10_000_000) * 100).ok()?;
    let unix_secs = secs_since_1601.checked_sub(WINDOWS_TO_UNIX_EPOCH_SECS)?;
    DateTime::from_timestamp(unix_secs, nanos)
}

/// Chromium 시각(마이크로초, 1601 기준) → UTC. 0 이하는 "값 없음".
pub fn webkit_micros_to_utc(micros: i64) -> Option<DateTime<Utc>> {
    if micros <= 0 {
        return None;
    }
    let secs_since_1601 = micros / 1_000_000;
    let nanos = u32::try_from((micros % 1_000_000) * 1_000).ok()?;
    let unix_secs = secs_since_1601.checked_sub(WINDOWS_TO_UNIX_EPOCH_SECS)?;
    DateTime::from_timestamp(unix_secs, nanos)
}

/// Firefox 시각(마이크로초, 1970 기준) → UTC. 0 이하는 "값 없음".
pub fn unix_micros_to_utc(micros: i64) -> Option<DateTime<Utc>> {
    if micros <= 0 {
        return None;
    }
    let secs = micros / 1_000_000;
    let nanos = u32::try_from((micros % 1_000_000) * 1_000).ok()?;
    DateTime::from_timestamp(secs, nanos)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn filetime_known_value() {
        // 유닉스 기준 시각(1970-01-01)은 FILETIME 116444736000000000 이다.
        let dt = filetime_to_utc(116_444_736_000_000_000).unwrap();
        assert_eq!(dt.to_rfc3339(), "1970-01-01T00:00:00+00:00");
        // 2021-02-08T12:41:58Z = 유닉스 1612788118초
        let ft = (1_612_788_118u64 + 11_644_473_600) * 10_000_000;
        assert_eq!(filetime_to_utc(ft).unwrap().to_rfc3339(), "2021-02-08T12:41:58+00:00");
    }

    #[test]
    fn zero_and_extreme_values_do_not_panic() {
        assert!(filetime_to_utc(0).is_none());
        let _ = filetime_to_utc(u64::MAX);
        assert!(webkit_micros_to_utc(0).is_none());
        let _ = webkit_micros_to_utc(i64::MAX);
        let _ = webkit_micros_to_utc(i64::MIN);
        let _ = unix_micros_to_utc(i64::MAX);
    }

    #[test]
    fn webkit_and_unix_agree() {
        let unix_micros = 1_612_788_118_000_000i64;
        let webkit = unix_micros + WINDOWS_TO_UNIX_EPOCH_SECS * 1_000_000;
        assert_eq!(webkit_micros_to_utc(webkit), unix_micros_to_utc(unix_micros));
    }
}
