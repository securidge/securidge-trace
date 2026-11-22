//! 전체 흐름 시험: 입력 폴더 만들기 → trace 실행 → 결과 확인.
//!
//! 확인하는 것
//! - 휴지통·바로 가기·브라우저 기록이 하나의 시간순 목록으로 합쳐지는가
//! - 같은 입력으로 두 번 실행하면 evidence.json이 완전히 같은가 (재현 가능)
//! - 분석이 입력 폴더를 바꾸지 않는가
//! - 출력 폴더가 입력 안에 있으면 거부하는가
//! - 깨진 파일이 섞여 있어도 끝까지 실행되고, 깨진 파일은 "읽지 못한 항목"으로 남는가

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-e2e-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

/// 2021-02-08T12:41:58Z
const UNIX_SECS: u64 = 1_612_788_118;

fn recycle_index_v2(path: &str, filetime: u64) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&2u64.to_le_bytes());
    d.extend_from_slice(&2048u64.to_le_bytes());
    d.extend_from_slice(&filetime.to_le_bytes());
    d.extend_from_slice(&(path.encode_utf16().count() as u32 + 1).to_le_bytes());
    d.extend(path.encode_utf16().flat_map(|u| u.to_le_bytes()));
    d.extend_from_slice(&[0, 0]);
    d
}

fn build_input(root: &Path) {
    let filetime = (UNIX_SECS + 11_644_473_600) * 10_000_000;
    let sid = "S-1-5-21-1111-2222-3333-1001";
    write(
        &root.join(format!("$Recycle.Bin/{sid}/$IAAAAAA.xlsx")),
        &recycle_index_v2("C:\\Users\\kim\\Desktop\\견적서.xlsx", filetime),
    );
    // 깨진 $I 파일
    write(&root.join(format!("$Recycle.Bin/{sid}/$IBROKEN.txt")), b"broken");

    // 실제 Windows가 만든 바로 가기 샘플
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../parsers/tests/data/lnk/WIN10/1607_14393/windows_generated.lnk");
    write(
        &root.join("Users/kim/AppData/Roaming/Microsoft/Windows/Recent/test.txt.lnk"),
        &fs::read(sample).unwrap(),
    );

    // Prefetch와 점프 목록 (공개 표본, crates/parsers/tests/data 참고)
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../parsers/tests/data");
    write(
        &root.join("Windows/Prefetch/CHROME.EXE-B3BA7868.pf"),
        &fs::read(data.join("ez-prefetch/CHROME.EXE-B3BA7868.pf")).unwrap(),
    );
    write(
        &root.join("Users/kim/AppData/Roaming/Microsoft/Windows/Recent/AutomaticDestinations/f01b4d95cf55d32a.automaticDestinations-ms"),
        &fs::read(data.join("ez-jumplist/f01b4d95cf55d32a.automaticDestinations-ms")).unwrap(),
    );

    // Chromium 기록
    let db = root.join("Users/kim/AppData/Local/Google/Chrome/User Data/Default/History");
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let webkit = (UNIX_SECS as i64 + 3600 + 11_644_473_600) * 1_000_000;
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch(&format!(
        "CREATE TABLE urls(id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR);
         CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
         CREATE TABLE downloads(id INTEGER PRIMARY KEY, target_path LONGVARCHAR,
            start_time INTEGER, end_time INTEGER, total_bytes INTEGER, state INTEGER, tab_url VARCHAR);
         INSERT INTO urls VALUES(1,'https://drive.example.com/','=HYPERLINK(1)');
         INSERT INTO visits VALUES(1,1,{webkit});
         INSERT INTO downloads VALUES(1,'C:\\Users\\kim\\Downloads\\tool.zip',{webkit},{webkit},5,1,'https://dl.example.com/');"
    ))
    .unwrap();
}

/// 폴더 안 모든 파일의 (상대 경로 → 내용) 스냅샷.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().display().to_string(), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn run_trace(input: &Path, output: &Path, lang: &str) -> (i32, String, PathBuf) {
    let out = Command::new(env!("CARGO_BIN_EXE_trace"))
        .args(["analyze", "--input"])
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(["--case", "고객사A PC-01", "--analyst", "홍길동", "--lang", lang])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let result_dir = fs::read_dir(output)
        .ok()
        .and_then(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).max())
        .unwrap_or_default();
    (out.status.code().unwrap_or(-1), stdout, result_dir)
}

#[test]
fn full_run_is_correct_reproducible_and_leaves_input_untouched() {
    let input = temp_dir("input");
    build_input(&input);
    let before = snapshot(&input);

    let out1 = temp_dir("out1");
    let (code, stdout, dir1) = run_trace(&input, &out1, "ko");
    // 깨진 파일이 하나 있으므로 종료 코드는 2 (끝났지만 읽지 못한 항목 있음)
    assert_eq!(code, 2, "stdout:\n{stdout}");
    assert!(stdout.contains("입력 파일은 분석 전후로 바뀌지 않았습니다"), "{stdout}");

    // 입력 폴더가 그대로인가 (새 파일이 생기지도, 내용이 바뀌지도 않아야 한다)
    assert_eq!(before, snapshot(&input), "분석이 입력 폴더를 바꿨음");

    // 결과 폴더 이름: <YYYYMMDD_HHMMSS>_<사건명>
    let dir_name = dir1.file_name().unwrap().to_string_lossy().into_owned();
    assert!(dir_name.starts_with("20") && dir_name.ends_with("_고객사A_PC-01"), "{dir_name}");
    // 보고서와 case.json
    let html = fs::read_to_string(dir1.join("report_ko.html")).unwrap();
    assert!(html.contains("PC 사용 기록 점검 보고서"));
    assert!(html.contains("판단 불가"), "SYSTEM 하이브가 없으므로 판단 불가");
    assert!(!html.contains("<script>alert"), "값은 이스케이프되어야 함");
    assert!(dir1.join("case.json").is_file());

    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(dir1.join("evidence.json")).unwrap()).unwrap();
    let events = evidence["events"].as_array().unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "FileDeleted").count(), 1);
    let from = |kind: &str, artifact: &str| {
        events.iter().filter(|e| e["kind"] == kind && e["source"]["artifact"] == artifact).count()
    };
    assert_eq!(from("FileOpened", "lnk"), 1);
    assert_eq!(from("FileOpened", "jumplist"), 9);
    assert_eq!(from("ProgramRun", "prefetch"), 8);
    assert!(html.contains("실행한 프로그램") && html.contains("CHROME.EXE"));
    assert_eq!(kinds.iter().filter(|k| **k == "WebVisit").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "WebDownload").count(), 1);

    // 삭제 기록: 경로, 시각, 계정(SID)
    let deleted = events.iter().find(|e| e["kind"] == "FileDeleted").unwrap();
    assert_eq!(deleted["subject"], "C:\\Users\\kim\\Desktop\\견적서.xlsx");
    assert_eq!(deleted["time"]["utc"], "2021-02-08T12:41:58Z");
    assert_eq!(deleted["account"]["sid"], "S-1-5-21-1111-2222-3333-1001");

    // 열람 기록: 볼륨 시리얼이 들어 있는가
    let opened = events.iter().find(|e| e["kind"] == "FileOpened" && e["source"]["artifact"] == "lnk").unwrap();
    assert_eq!(opened["details"]["volume_serial"], "E02E-8A93");
    assert_eq!(opened["details"]["drive_type"], "fixed");
    assert_eq!(opened["time"]["basis"], "FileSystem");

    // 시각이 있는 기록은 시간순이어야 한다 (삭제 12:41 → 웹 13:41)
    let pos = |k: &str| kinds.iter().position(|x| *x == k).unwrap();
    assert!(pos("FileDeleted") < pos("WebVisit"));

    // 깨진 파일은 '읽지 못한 항목'에 남는다
    let issues = evidence["issues"].as_array().unwrap();
    assert!(issues.iter().any(|i| i["file"].as_str().unwrap().contains("$IBROKEN")));

    // 근거 파일에는 실행할 때마다 달라지는 값(절대 경로 등)이 없어야 한다
    let text = fs::read_to_string(dir1.join("evidence.json")).unwrap();
    assert!(!text.contains(&input.display().to_string()));

    // CSV: BOM으로 시작, 요청한 시각 형식, 수식처럼 보이는 값은 무력화
    let csv = fs::read(dir1.join("timeline.csv")).unwrap();
    assert_eq!(&csv[..3], &[0xEF, 0xBB, 0xBF]);
    let csv_text = String::from_utf8_lossy(&csv);
    assert!(csv_text.contains("2021.02.08.(월), 21:41:58 (KST, UTC+09:00)"), "{csv_text}");

    // 해시 목록이 실제 파일과 맞는가
    let sums = fs::read_to_string(dir1.join("SHA256SUMS.txt")).unwrap();
    assert_eq!(sums.lines().count(), 5, "{sums}"); // evidence, case, timeline, report_ko, report_en
    assert!(dir1.join("report_en.html").exists());
    assert!(st_report::verify_sums(&dir1).unwrap().is_empty());

    // 보고서의 실행 시각 형식: 2026.11.21.(토), 18:01:02 (KST, UTC+09:00)
    let created = html.split("보고서 생성</td><td>").nth(1).unwrap().split('<').next().unwrap();
    let chars: Vec<char> = created.chars().collect();
    assert_eq!(chars[4], '.');
    assert_eq!(chars[10], '.');
    assert_eq!(chars[11], '(');
    assert!(created.ends_with("(KST, UTC+09:00)"), "{created}");
    let case: serde_json::Value = serde_json::from_slice(&fs::read(dir1.join("case.json")).unwrap()).unwrap();
    assert_eq!(case["info"]["input_unchanged"], true);

    // 다른 언어로 다시 만들기 (재수집 없음)
    let st = Command::new(env!("CARGO_BIN_EXE_trace")).args(["regen", "--dir"]).arg(&dir1).args(["--lang", "en"]).status().unwrap();
    assert!(st.success());
    let en = fs::read_to_string(dir1.join("report_en.html")).unwrap();
    assert!(en.contains("PC Activity Review Report") && en.contains("Report regenerated"));
    assert!(st_report::verify_sums(&dir1).unwrap().is_empty());

    // 두 번째 실행(영어): 근거 파일은 언어와 무관하게 완전히 같아야 한다
    let out2 = temp_dir("out2");
    let (code2, stdout2, dir2) = run_trace(&input, &out2, "en");
    assert_eq!(code2, 2);
    assert!(stdout2.contains("Input files were not changed"), "{stdout2}");
    assert!(dir2.join("report_en.html").is_file());
    assert_eq!(
        fs::read(dir1.join("evidence.json")).unwrap(),
        fs::read(dir2.join("evidence.json")).unwrap(),
        "같은 입력인데 evidence.json이 다름"
    );
    let csv2 = String::from_utf8_lossy(&fs::read(dir2.join("timeline.csv")).unwrap()).into_owned();
    assert!(csv2.contains("2021.02.08.(Mon), 21:41:58 (KST, UTC+09:00)"), "{csv2}");
}

#[test]
fn refuses_output_inside_input() {
    let input = temp_dir("input-refuse");
    build_input(&input);
    let before = snapshot(&input);
    let (code, _, _) = run_trace(&input, &input.join("reports"), "ko");
    assert_eq!(code, 1);
    assert_eq!(before, snapshot(&input), "거부했는데도 입력 폴더에 무언가 생김");
}

#[test]
fn missing_input_is_an_error() {
    let out = temp_dir("out-missing");
    let (code, _, _) = run_trace(Path::new("/no/such/input/folder"), &out, "ko");
    assert_eq!(code, 1);
}

#[test]
fn clean_input_produces_no_events_and_exit_zero() {
    let input = temp_dir("input-clean");
    fs::create_dir_all(input.join("Users/kim")).unwrap();
    let out = temp_dir("out-clean");
    let (code, stdout, dir) = run_trace(&input, &out, "ko");
    assert_eq!(code, 0, "{stdout}");
    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("evidence.json")).unwrap()).unwrap();
    assert_eq!(evidence["events"].as_array().unwrap().len(), 0);
    // 못 찾은 아티팩트도 '0개'로 목록에 남아야 한다 (기록 없음과 행위 없음의 구분)
    assert_eq!(evidence["checks"].as_array().unwrap().len(), 18);
}
