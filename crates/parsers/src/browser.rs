//! 브라우저 기록 파서 (방문 기록, 다운로드 기록).
//!
//! - Chromium 계열(Chrome, Edge, Whale 등): 프로필 폴더의 `History` (SQLite)
//!   - `urls`(id, url, title), `visits`(id, url → urls.id, visit_time)
//!   - `downloads`(id, target_path, start_time, end_time, total_bytes, state, tab_url …),
//!     `downloads_url_chains`(id, chain_index, url)
//!   - 시각: 1601-01-01 UTC 기준 마이크로초
//!   - 근거: Chromium 소스 components/history/core/browser/ (url_database.cc,
//!     visit_database.cc, download_database.cc)
//! - Firefox: 프로필 폴더의 `places.sqlite`
//!   - `moz_places`(id, url, title), `moz_historyvisits`(id, place_id, visit_date)
//!   - 시각: 1970-01-01 UTC 기준 마이크로초
//!   - 근거: Firefox 소스 toolkit/components/places/ (nsPlacesTables.h)
//!
//! 이 파서는 **작업용 사본**을 연다. 원본을 직접 열지 않는 이유는 SQLite가 원본 옆에
//! 보조 파일(-wal, -shm)을 만들 수 있기 때문이다. 사본을 만드는 일은 호출하는 쪽이 한다.
//!
//! 쿠키, 저장된 비밀번호 등 암호화된 데이터는 다루지 않는다.
//!
//! 검증 상태: 위 근거의 표 구조로 만든 합성 DB로만 테스트함. **실제 브라우저가 만든 DB로는
//! 미검증.** 브라우저 버전에 따라 열 이름이 다를 수 있어, 있는 열만 골라 읽도록 했다.
//! Firefox 다운로드 기록은 아직 읽지 않는다 (Phase 2 과제).

use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use st_core::time::{unix_micros_to_utc, webkit_micros_to_utc};

/// 표 하나에서 읽는 최대 행 수. 비정상적으로 큰 DB가 메모리를 다 쓰지 못하게 한다.
pub const MAX_ROWS: usize = 2_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebVisit {
    pub record_id: i64,
    pub url: String,
    pub title: String,
    pub time_raw: i64,
    pub time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebDownload {
    pub record_id: i64,
    pub target_path: String,
    pub url: String,
    pub start_raw: i64,
    pub start: Option<DateTime<Utc>>,
    pub end_raw: i64,
    pub end: Option<DateTime<Utc>>,
    pub total_bytes: i64,
    pub state: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BrowserHistory {
    pub visits: Vec<WebVisit>,
    pub downloads: Vec<WebDownload>,
    /// 일부만 읽었거나 건너뛴 부분에 대한 설명.
    pub notes: Vec<String>,
}

fn open(path: &Path) -> Result<Connection, String> {
    // 사본이므로 읽기·쓰기로 연다 (WAL 방식 DB는 읽기 전용으로 열리지 않는 경우가 있다).
    // 대신 query_only를 켜서 이 연결이 내용을 바꾸지 못하게 한다.
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(|e| format!("DB 열기 실패: {e}"))?;
    conn.execute_batch("PRAGMA query_only = ON;")
        .map_err(|e| format!("DB 설정 실패: {e}"))?;
    Ok(conn)
}

/// 표에 있는 열 이름 목록. 표가 없으면 빈 집합.
fn columns(conn: &Connection, table: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    // 표 이름은 이 파일 안의 상수만 들어오므로 문자열 조립이 안전하다.
    let sql = format!("PRAGMA table_info({table})");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return set;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return set;
    };
    for name in rows.flatten() {
        set.insert(name);
    }
    set
}

/// Chromium 계열 `History` 사본을 읽는다.
pub fn read_chromium(path: &Path) -> Result<BrowserHistory, String> {
    let conn = open(path)?;
    let mut out = BrowserHistory::default();

    // --- 방문 기록 ---
    let has_visits = !columns(&conn, "visits").is_empty() && !columns(&conn, "urls").is_empty();
    if has_visits {
        let sql = "SELECT v.id, u.url, u.title, v.visit_time \
                   FROM visits v JOIN urls u ON v.url = u.id ORDER BY v.id";
        match conn.prepare(sql) {
            Ok(mut stmt) => {
                let rows = stmt.query_map([], |row| {
                    let raw: i64 = row.get(3)?;
                    Ok(WebVisit {
                        record_id: row.get(0)?,
                        url: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        time_raw: raw,
                        time: webkit_micros_to_utc(raw),
                    })
                });
                collect_rows(rows, &mut out.visits, "visits", &mut out.notes);
            }
            Err(e) => out.notes.push(format!("visits 읽기 실패: {e}")),
        }
    } else {
        out.notes.push("방문 기록 표(visits/urls)가 없음".to_string());
    }

    // --- 다운로드 기록 ---
    let dl_cols = columns(&conn, "downloads");
    if dl_cols.is_empty() {
        out.notes.push("다운로드 기록 표(downloads)가 없음".to_string());
    } else {
        // 버전마다 열 구성이 달라질 수 있어, 없는 열은 NULL로 대신한다.
        let col = |name: &'static str| -> &'static str {
            if dl_cols.contains(name) {
                name
            } else {
                "NULL"
            }
        };
        let has_chain = !columns(&conn, "downloads_url_chains").is_empty();
        let url_expr = if has_chain {
            "(SELECT c.url FROM downloads_url_chains c WHERE c.id = d.id \
              ORDER BY c.chain_index DESC LIMIT 1)"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT d.id, {target}, {start}, {end}, {total}, {state}, {tab}, {url_expr} \
             FROM downloads d ORDER BY d.id",
            target = col("target_path"),
            start = col("start_time"),
            end = col("end_time"),
            total = col("total_bytes"),
            state = col("state"),
            tab = col("tab_url"),
        );
        match conn.prepare(&sql) {
            Ok(mut stmt) => {
                let rows = stmt.query_map([], |row| {
                    let start_raw = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
                    let end_raw = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
                    let tab_url = row.get::<_, Option<String>>(6)?.unwrap_or_default();
                    let chain_url = row.get::<_, Option<String>>(7)?.unwrap_or_default();
                    Ok(WebDownload {
                        record_id: row.get(0)?,
                        target_path: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        url: if chain_url.is_empty() { tab_url } else { chain_url },
                        start_raw,
                        start: webkit_micros_to_utc(start_raw),
                        end_raw,
                        end: webkit_micros_to_utc(end_raw),
                        total_bytes: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                        state: row.get::<_, Option<i64>>(5)?,
                    })
                });
                collect_rows(rows, &mut out.downloads, "downloads", &mut out.notes);
            }
            Err(e) => out.notes.push(format!("downloads 읽기 실패: {e}")),
        }
    }

    Ok(out)
}

/// Firefox `places.sqlite` 사본을 읽는다 (방문 기록만).
pub fn read_firefox(path: &Path) -> Result<BrowserHistory, String> {
    let conn = open(path)?;
    let mut out = BrowserHistory::default();

    let has_tables = !columns(&conn, "moz_historyvisits").is_empty()
        && !columns(&conn, "moz_places").is_empty();
    if !has_tables {
        out.notes.push("방문 기록 표(moz_historyvisits/moz_places)가 없음".to_string());
        return Ok(out);
    }
    let sql = "SELECT v.id, p.url, p.title, v.visit_date \
               FROM moz_historyvisits v JOIN moz_places p ON v.place_id = p.id ORDER BY v.id";
    match conn.prepare(sql) {
        Ok(mut stmt) => {
            let rows = stmt.query_map([], |row| {
                let raw = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
                Ok(WebVisit {
                    record_id: row.get(0)?,
                    url: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    time_raw: raw,
                    time: unix_micros_to_utc(raw),
                })
            });
            collect_rows(rows, &mut out.visits, "moz_historyvisits", &mut out.notes);
        }
        Err(e) => out.notes.push(format!("moz_historyvisits 읽기 실패: {e}")),
    }
    out.notes.push("Firefox 다운로드 기록은 아직 지원하지 않음".to_string());
    Ok(out)
}

/// 행을 모으되, 읽지 못한 행은 세어서 notes에 남기고 계속한다.
fn collect_rows<T, I>(
    rows: rusqlite::Result<I>,
    into: &mut Vec<T>,
    table: &str,
    notes: &mut Vec<String>,
) where
    I: Iterator<Item = rusqlite::Result<T>>,
{
    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            notes.push(format!("{table} 읽기 실패: {e}"));
            return;
        }
    };
    let mut bad = 0usize;
    for row in rows {
        if into.len() >= MAX_ROWS {
            notes.push(format!("{table}: 행이 너무 많아 {MAX_ROWS}개까지만 읽음"));
            break;
        }
        match row {
            Ok(v) => into.push(v),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        notes.push(format!("{table}: 읽지 못한 행 {bad}개"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 2021-02-08T12:41:58Z 를 Chromium 시각으로.
    const WEBKIT: i64 = (1_612_788_118 + 11_644_473_600) * 1_000_000;

    fn temp_db(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("st-browser-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("db")
    }

    fn make_chromium(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(&format!(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR,
                visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER,
                from_visit INTEGER, transition INTEGER, segment_id INTEGER, visit_duration INTEGER);
             CREATE TABLE downloads(id INTEGER PRIMARY KEY, guid VARCHAR, current_path LONGVARCHAR,
                target_path LONGVARCHAR, start_time INTEGER, received_bytes INTEGER,
                total_bytes INTEGER, state INTEGER, end_time INTEGER, tab_url VARCHAR);
             CREATE TABLE downloads_url_chains(id INTEGER, chain_index INTEGER, url LONGVARCHAR);
             INSERT INTO urls VALUES(1,'https://drive.example.com/upload','드라이브',1,0,{w},0);
             INSERT INTO visits VALUES(10,1,{w},0,0,0,0);
             INSERT INTO visits VALUES(11,1,0,0,0,0,0);
             INSERT INTO downloads VALUES(5,'g','C:\\Users\\kim\\Downloads\\a.zip',
                'C:\\Users\\kim\\Downloads\\a.zip',{w},10,10,1,{w},'https://tab.example.com/');
             INSERT INTO downloads_url_chains VALUES(5,0,'https://first.example.com/a');
             INSERT INTO downloads_url_chains VALUES(5,1,'https://cdn.example.com/a.zip');",
            w = WEBKIT
        ))
        .unwrap();
    }

    #[test]
    fn reads_chromium_visits_and_downloads() {
        let db = temp_db("chromium");
        make_chromium(&db);
        let h = read_chromium(&db).unwrap();
        assert_eq!(h.visits.len(), 2);
        let first = h.visits.first().unwrap();
        assert_eq!(first.url, "https://drive.example.com/upload");
        assert_eq!(first.title, "드라이브");
        assert_eq!(first.time.unwrap().to_rfc3339(), "2021-02-08T12:41:58+00:00");
        // 시각이 0인 방문은 "값 없음"으로 남는다.
        assert_eq!(h.visits.get(1).unwrap().time, None);

        assert_eq!(h.downloads.len(), 1);
        let d = h.downloads.first().unwrap();
        assert_eq!(d.target_path, "C:\\Users\\kim\\Downloads\\a.zip");
        // 여러 번 넘겨진 주소 중 마지막 주소를 쓴다.
        assert_eq!(d.url, "https://cdn.example.com/a.zip");
        assert_eq!(d.total_bytes, 10);
    }

    #[test]
    fn chromium_with_missing_tables_or_columns_does_not_fail() {
        let db = temp_db("partial");
        let c = Connection::open(&db).unwrap();
        // 열이 거의 없는 downloads 표, visits 표는 아예 없음
        c.execute_batch(
            "CREATE TABLE downloads(id INTEGER PRIMARY KEY, target_path LONGVARCHAR);
             INSERT INTO downloads VALUES(1,'C:\\x.bin');",
        )
        .unwrap();
        drop(c);
        let h = read_chromium(&db).unwrap();
        assert!(h.visits.is_empty());
        assert_eq!(h.downloads.len(), 1);
        assert_eq!(h.downloads.first().unwrap().start, None);
        assert!(h.notes.iter().any(|n| n.contains("visits")));
    }

    #[test]
    fn reads_firefox_visits() {
        let db = temp_db("firefox");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR);
             CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY, from_visit INTEGER,
                place_id INTEGER, visit_date INTEGER, visit_type INTEGER, session INTEGER);
             INSERT INTO moz_places VALUES(1,'https://mail.example.com/',NULL);
             INSERT INTO moz_historyvisits VALUES(7,0,1,1612788118000000,1,0);",
        )
        .unwrap();
        drop(c);
        let h = read_firefox(&db).unwrap();
        assert_eq!(h.visits.len(), 1);
        let v = h.visits.first().unwrap();
        assert_eq!(v.title, "");
        assert_eq!(v.time.unwrap().to_rfc3339(), "2021-02-08T12:41:58+00:00");
    }

    #[test]
    fn non_database_file_is_an_error_not_a_panic() {
        let db = temp_db("garbage");
        std::fs::write(&db, b"this is not a sqlite file at all").unwrap();
        // 열기 자체가 실패하거나, 열려도 표가 없다고 나와야 한다.
        match read_chromium(&db) {
            Ok(h) => assert!(h.visits.is_empty() && h.downloads.is_empty()),
            Err(e) => assert!(!e.is_empty()),
        }
    }
}
