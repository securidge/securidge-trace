//! SRUDB.dat 안의 표별 기록 수와 네트워크 사용량 합계를 출력한다 (실기 검증용).
//! 사용: cargo run --release -p st-parsers --example probe_srum -- <SRUDB.dat>
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("SRUDB.dat 경로");
    let data = std::fs::read(&path).expect("읽기");
    match srum::read(&data) {
        Ok(db) => {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for e in &db.entries {
                *counts.entry(e.table.clone()).or_default() += 1;
            }
            println!("SRUM 표별 기록 수:");
            for (t, n) in &counts {
                println!("  {t}: {n}");
            }
            println!("문제 {}개: {:?}", db.problems.len(), db.problems.iter().take(5).collect::<Vec<_>>());
        }
        Err(e) => println!("SRUM 읽기 실패: {e}"),
    }
    match st_parsers::srum::read(&data) {
        Ok(r) => {
            println!("네트워크 사용량(날짜×프로그램): {}개, 기간 {:?} ~ {:?}", r.usage.len(), r.earliest, r.latest);
            for u in r.usage.iter().take(5) {
                println!("  {} {} 보냄 {} 받음 {}", u.day, u.app, u.bytes_sent, u.bytes_received);
            }
        }
        Err(e) => println!("요약 실패: {e}"),
    }
}
