//! 레지스트리 하이브 파일을 읽는 얇은 도구.
//!
//! 실제 해석은 notatin crate(Stroz Friedberg, Apache-2.0)가 한다. 이 모듈은
//! - 같은 폴더의 트랜잭션 로그(.LOG1/.LOG2)를 찾아 함께 적용하고 (아직 하이브에 반영되지 않은 최근 변경분)
//! - 경로로 키를 찾고, 값을 문자열·숫자·바이트로 꺼내는 일만 한다.
//!
//! 키 경로는 루트 이름 없이 쓴다. 예: `ControlSet001\Enum\USBSTOR`
//! 대소문자는 구분하지 않는다 (notatin이 처리).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use notatin::cell_key_node::CellKeyNode;
use notatin::cell_value::CellValue;
use notatin::parser::Parser;
use notatin::parser_builder::ParserBuilder;

pub struct Hive {
    parser: Parser,
    /// 트랜잭션 로그를 적용했는지, 못 했다면 왜인지.
    pub log_note: Option<String>,
}

/// 키 하나. 이름, 마지막 기록 시각, 하위 키·값에 접근한다.
pub struct Key {
    node: CellKeyNode,
}

/// 하이브 옆의 트랜잭션 로그 파일 (대소문자 무시).
pub fn transaction_logs(primary: &Path) -> Vec<PathBuf> {
    let Some(dir) = primary.parent() else { return Vec::new() };
    let Some(name) = primary.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            if n == format!("{name}.log1") || n == format!("{name}.log2") {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

impl Hive {
    /// 하이브를 연다. 트랜잭션 로그가 있으면 적용을 시도하고, 실패하면 로그 없이 다시 연다.
    // notatin은 'static 경로를 요구하므로 to_path_buf가 필요하다.
    #[allow(clippy::unnecessary_to_owned)]
    pub fn open(path: &Path) -> Result<Hive, String> {
        let logs = transaction_logs(path);
        if !logs.is_empty() {
            let mut b = ParserBuilder::from_path(path.to_path_buf());
            for l in &logs {
                b.with_transaction_log(l.clone());
            }
            match b.build() {
                Ok(parser) => {
                    return Ok(Hive { parser, log_note: Some(format!("트랜잭션 로그 {}개 적용", logs.len())) })
                }
                Err(e) => {
                    let parser = ParserBuilder::from_path(path.to_path_buf())
                        .build()
                        .map_err(|e2| format!("하이브를 열지 못함: {e2}"))?;
                    return Ok(Hive {
                        parser,
                        log_note: Some(format!("트랜잭션 로그 적용 실패, 최근 변경이 빠졌을 수 있음: {e}")),
                    });
                }
            }
        }
        let parser = ParserBuilder::from_path(path.to_path_buf())
            .build()
            .map_err(|e| format!("하이브를 열지 못함: {e}"))?;
        Ok(Hive {
            parser,
            log_note: Some("트랜잭션 로그 없음 (최근 변경이 빠졌을 수 있음)".to_string()),
        })
    }

    /// 경로의 키. 없으면 None.
    pub fn key(&mut self, path: &str) -> Option<Key> {
        self.parser.get_key(path, false).ok().flatten().map(|node| Key { node })
    }

    /// 하위 키 목록 (이름순).
    pub fn subkeys(&mut self, key: &mut Key) -> Vec<Key> {
        let mut v: Vec<Key> =
            key.node.read_sub_keys(&mut self.parser).into_iter().map(|node| Key { node }).collect();
        v.sort_by_key(|k| k.name().to_lowercase());
        v
    }

    /// 하위 키 하나 (상대 경로).
    pub fn subkey(&mut self, key: &mut Key, rel: &str) -> Option<Key> {
        key.node.get_sub_key_by_path(&mut self.parser, rel).map(|node| Key { node })
    }

    /// 현재 사용 중인 ControlSet 이름 (SYSTEM 하이브). `Select\Current` 값으로 정한다.
    pub fn current_control_set(&mut self) -> String {
        let n = self.key("Select").and_then(|k| k.u32("Current")).unwrap_or(1);
        format!("ControlSet{n:03}")
    }
}

impl Key {
    pub fn name(&self) -> &str {
        &self.node.key_name
    }

    pub fn last_written(&self) -> DateTime<Utc> {
        self.node.last_key_written_date_and_time()
    }

    fn raw(&self, name: &str) -> Option<CellValue> {
        let v = self.node.get_value(name)?;
        Some(v.get_content().0)
    }

    /// 문자열 값 (REG_SZ, REG_EXPAND_SZ). REG_MULTI_SZ는 첫 줄.
    pub fn string(&self, name: &str) -> Option<String> {
        match self.raw(name)? {
            CellValue::String(s) => Some(s.trim_end_matches('\0').to_string()),
            CellValue::MultiString(v) => v.into_iter().next(),
            _ => None,
        }
    }

    pub fn multi_string(&self, name: &str) -> Vec<String> {
        match self.raw(name) {
            Some(CellValue::MultiString(v)) => v,
            Some(CellValue::String(s)) => vec![s],
            _ => Vec::new(),
        }
    }

    pub fn u32(&self, name: &str) -> Option<u32> {
        match self.raw(name)? {
            CellValue::U32(v) => Some(v),
            CellValue::I32(v) => Some(v as u32),
            CellValue::U64(v) => u32::try_from(v).ok(),
            CellValue::Binary(b) => st_core::bytes::u32_le(&b, 0),
            _ => None,
        }
    }

    pub fn u64(&self, name: &str) -> Option<u64> {
        match self.raw(name)? {
            CellValue::U64(v) => Some(v),
            CellValue::I64(v) => Some(v as u64),
            CellValue::U32(v) => Some(u64::from(v)),
            CellValue::Binary(b) => st_core::bytes::u64_le(&b, 0),
            _ => None,
        }
    }

    pub fn binary(&self, name: &str) -> Option<Vec<u8>> {
        match self.raw(name)? {
            CellValue::Binary(b) => Some(b),
            _ => None,
        }
    }

    /// 모든 값 (이름, 바이트). 바이트가 아닌 값은 건너뛴다.
    pub fn binary_values(&self) -> Vec<(String, Vec<u8>)> {
        self.node
            .value_iter()
            .filter_map(|v| match v.get_content().0 {
                CellValue::Binary(b) => Some((v.get_pretty_name(), b)),
                _ => None,
            })
            .collect()
    }

    /// 문자열·숫자 값을 문자열로 (값 종류를 가리지 않고 읽을 때).
    pub fn text(&self, name: &str) -> Option<String> {
        match self.raw(name)? {
            CellValue::String(s) => Some(s.trim_end_matches('\0').to_string()),
            CellValue::MultiString(v) => v.into_iter().next(),
            CellValue::U32(v) => Some(v.to_string()),
            CellValue::I32(v) => Some(v.to_string()),
            CellValue::U64(v) => Some(v.to_string()),
            CellValue::I64(v) => Some(v.to_string()),
            _ => None,
        }
    }

    /// 값 이름 목록.
    pub fn value_names(&self) -> Vec<String> {
        self.node.value_iter().map(|v| v.get_pretty_name()).collect()
    }

    /// 이 키의 기본값 또는 첫 번째 8바이트 값을 FILETIME으로 (장치 속성 키용).
    pub fn first_filetime(&self) -> Option<u64> {
        for v in self.node.value_iter() {
            match v.get_content().0 {
                CellValue::Binary(b) if b.len() == 8 => return st_core::bytes::u64_le(&b, 0),
                CellValue::U64(x) => return Some(x),
                _ => {}
            }
        }
        None
    }
}
