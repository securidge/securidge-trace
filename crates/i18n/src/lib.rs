//! st-i18n: 한국어·영어 문구와 시각 표기.
//!
//! - 문구는 `locales/ko.json`, `locales/en.json`에만 둔다. 코드에 문구를 직접 쓰지 않는다.
//! - 두 파일의 키가 완전히 같은지 테스트가 확인한다 (한쪽 언어에만 있는 문구 방지).
//! - 시각 표기는 개발 요청서 9.1을 따른다: `2026.11.21.(토), 18:01:02`

use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::{DateTime, Datelike, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

const KO_JSON: &str = include_str!("../locales/ko.json");
const EN_JSON: &str = include_str!("../locales/en.json");

/// 표시 언어.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Ko,
    En,
}

impl Lang {
    /// "ko", "en" (대소문자 무시). 그 밖의 값은 None.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "ko" | "kr" | "korean" => Some(Lang::Ko),
            "en" | "english" => Some(Lang::En),
            _ => None,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
        }
    }
}

type Catalog = BTreeMap<String, String>;

fn catalog(lang: Lang) -> &'static Catalog {
    static KO: OnceLock<Catalog> = OnceLock::new();
    static EN: OnceLock<Catalog> = OnceLock::new();
    match lang {
        // 내장 파일이 잘못돼도 프로그램이 죽지 않도록 빈 목록으로 대체한다.
        // 파일이 올바른지는 테스트(`catalogs_parse_and_match`)가 보장한다.
        Lang::Ko => KO.get_or_init(|| serde_json::from_str(KO_JSON).unwrap_or_default()),
        Lang::En => EN.get_or_init(|| serde_json::from_str(EN_JSON).unwrap_or_default()),
    }
}

/// 키에 해당하는 문구. 키가 없으면 키 자체를 돌려준다 (빈 화면보다 낫다).
pub fn t(lang: Lang, key: &str) -> String {
    catalog(lang).get(key).cloned().unwrap_or_else(|| key.to_string())
}

fn weekday_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "weekday.mon",
        Weekday::Tue => "weekday.tue",
        Weekday::Wed => "weekday.wed",
        Weekday::Thu => "weekday.thu",
        Weekday::Fri => "weekday.fri",
        Weekday::Sat => "weekday.sat",
        Weekday::Sun => "weekday.sun",
    }
}

/// 표시용 시각: `2026.11.21.(토), 18:01:02` / `2026.11.21.(Sat), 18:01:02`
pub fn format_stamp(utc: DateTime<Utc>, tz: Tz, lang: Lang) -> String {
    let local = utc.with_timezone(&tz);
    format!(
        "{}({}), {}",
        local.format("%Y.%m.%d."),
        t(lang, weekday_key(local.weekday())),
        local.format("%H:%M:%S")
    )
}

/// 시간대까지 붙인 표시용 시각: `2026.11.21.(토), 18:01:02 (KST, UTC+09:00)`
pub fn format_stamp_with_zone(utc: DateTime<Utc>, tz: Tz, lang: Lang) -> String {
    let local = utc.with_timezone(&tz);
    format!(
        "{} ({}, UTC{})",
        format_stamp(utc, tz, lang),
        local.format("%Z"),
        local.format("%:z")
    )
}

/// 파일 이름용 시각: `20261121_180102`
pub fn format_file_stamp(utc: DateTime<Utc>, tz: Tz) -> String {
    utc.with_timezone(&tz).format("%Y%m%d_%H%M%S").to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn catalogs_parse_and_match() {
        let ko: Catalog = serde_json::from_str(KO_JSON).unwrap();
        let en: Catalog = serde_json::from_str(EN_JSON).unwrap();
        let ko_keys: Vec<_> = ko.keys().collect();
        let en_keys: Vec<_> = en.keys().collect();
        assert_eq!(ko_keys, en_keys, "ko.json과 en.json의 키가 다릅니다");
        assert!(ko.values().all(|v| !v.trim().is_empty()));
        assert!(en.values().all(|v| !v.trim().is_empty()));
    }

    #[test]
    fn stamp_matches_requested_format() {
        // 2026-11-21 18:01:02 KST = 09:01:02 UTC, 토요일
        let utc = Utc.with_ymd_and_hms(2026, 11, 21, 9, 1, 2).unwrap();
        let seoul = chrono_tz::Asia::Seoul;
        assert_eq!(format_stamp(utc, seoul, Lang::Ko), "2026.11.21.(토), 18:01:02");
        assert_eq!(format_stamp(utc, seoul, Lang::En), "2026.11.21.(Sat), 18:01:02");
        assert_eq!(
            format_stamp_with_zone(utc, seoul, Lang::Ko),
            "2026.11.21.(토), 18:01:02 (KST, UTC+09:00)"
        );
        assert_eq!(format_file_stamp(utc, seoul), "20261121_180102");
    }

    #[test]
    fn date_rolls_over_with_time_zone() {
        // UTC로는 금요일 밤이지만 서울에서는 토요일 새벽이다.
        let utc = Utc.with_ymd_and_hms(2026, 11, 20, 16, 30, 0).unwrap();
        assert_eq!(
            format_stamp(utc, chrono_tz::Asia::Seoul, Lang::Ko),
            "2026.11.21.(토), 01:30:00"
        );
        assert_eq!(format_stamp(utc, chrono_tz::UTC, Lang::Ko), "2026.11.20.(금), 16:30:00");
    }

    #[test]
    fn missing_key_falls_back_to_key() {
        assert_eq!(t(Lang::Ko, "no.such.key"), "no.such.key");
    }
}
