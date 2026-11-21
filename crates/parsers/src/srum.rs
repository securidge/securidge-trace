//! SRUM(시스템 자원 사용 기록) — 프로그램별 네트워크 전송량.
//!
//! Windows는 `C:\Windows\System32\sru\SRUDB.dat`에 약 한 달 동안 프로그램별 자원 사용량을 한 시간
//! 단위로 기록한다. 이 중 "네트워크 사용량" 표에는 프로그램별로 보낸 바이트·받은 바이트가 있어,
//! **많은 양을 밖으로 보낸 프로그램**을 찾는 거의 유일한 흔적이다.
//!
//! 데이터베이스(ESE) 해석은 sootmark-srum crate(MIT/Apache-2.0, unsafe 금지, 외부 의존 없음)가 한다.
//! 여기서는 네트워크 사용량만 골라 "날짜(UTC) × 프로그램 × 사용자" 단위로 합친다.
//!
//! 한계
//! - Windows가 한 시간 남짓마다 디스크에 기록하므로, 가장 최근 한두 시간은 빠질 수 있다.
//! - 보낸 양이 많다는 것은 "무엇을 보냈는지"를 알려주지 않는다 (화상 회의, 백업, 업데이트일 수도 있다).
//! - 표의 시각은 "그 한 시간 집계가 기록된 시각"이다.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use common::time::Ts;
use serde::{Deserialize, Serialize};

/// 네트워크 사용량 표
const NETWORK_USAGE: &str = "{973F5D5C-1D90-4944-BE8E-24B94231A174}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetUsageDay {
    pub day: NaiveDate,
    pub app: String,
    pub user_sid: Option<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// 그 날 가장 늦은 집계 시각
    pub last_record: Option<DateTime<Utc>>,
    pub records: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SrumRead {
    pub usage: Vec<NetUsageDay>,
    pub earliest: Option<DateTime<Utc>>,
    pub latest: Option<DateTime<Utc>>,
    pub notes: Vec<String>,
}

fn to_utc(ts: &Ts) -> Option<DateTime<Utc>> {
    let s = ts.to_iso8601()?;
    if !s.ends_with('Z') {
        return None;
    }
    DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))
}

/// SRUDB.dat 내용을 읽어 날짜별 네트워크 사용량을 만든다.
pub fn read(data: &[u8]) -> Result<SrumRead, String> {
    let db = srum::read(data).map_err(|e| format!("SRUM 데이터베이스를 읽지 못함: {e}"))?;
    let mut groups: BTreeMap<(NaiveDate, String, Option<String>), NetUsageDay> = BTreeMap::new();
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut latest: Option<DateTime<Utc>> = None;
    for e in db.entries.iter().filter(|e| e.table.eq_ignore_ascii_case(NETWORK_USAGE)) {
        let Some(t) = e.time.as_ref().and_then(to_utc) else { continue };
        earliest = Some(earliest.map_or(t, |x| x.min(t)));
        latest = Some(latest.map_or(t, |x| x.max(t)));
        let sent = e.value("BytesSent").and_then(|v| v.as_i64()).and_then(|v| u64::try_from(v).ok()).unwrap_or(0);
        let recv = e.value("BytesRecvd").and_then(|v| v.as_i64()).and_then(|v| u64::try_from(v).ok()).unwrap_or(0);
        let app = e.app.clone().unwrap_or_else(|| format!("(앱 번호 {})", e.app_id.unwrap_or(-1)));
        let key = (t.date_naive(), app.clone(), e.user.clone());
        let g = groups.entry(key).or_insert_with(|| NetUsageDay {
            day: t.date_naive(),
            app,
            user_sid: e.user.clone(),
            bytes_sent: 0,
            bytes_received: 0,
            last_record: None,
            records: 0,
        });
        g.bytes_sent = g.bytes_sent.saturating_add(sent);
        g.bytes_received = g.bytes_received.saturating_add(recv);
        g.last_record = Some(g.last_record.map_or(t, |x| x.max(t)));
        g.records += 1;
    }
    let mut notes: Vec<String> = db.problems.into_iter().take(20).map(|p| format!("참고: {p}")).collect();
    if groups.is_empty() {
        notes.push("참고: 네트워크 사용량 기록이 없음".to_string());
    }
    Ok(SrumRead { usage: groups.into_values().collect(), earliest, latest, notes })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_an_error() {
        assert!(read(b"not an ese database").is_err());
        assert!(read(&[0u8; 8192]).is_err());
    }
}
