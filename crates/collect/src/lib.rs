//! st-collect: 입력에서 아티팩트 파일을 찾는다.
//!
//! 지금은 "수집해 둔 자료 분석"(오프라인 입력)만 지원한다. 입력은 Windows의 원래 폴더
//! 구조를 유지한 폴더이거나, 읽기 전용으로 마운트한 디스크 이미지의 드라이브 경로다.
//! 실행 중인 PC에서 잠긴 파일을 읽어 오는 기능은 Phase 3 과제다.
//!
//! 원칙
//! - 입력은 읽기만 한다. 이 crate에는 입력 경로에 쓰는 코드가 없다.
//! - 심볼릭 링크는 따라가지 않는다 (입력 폴더 밖으로 나가는 것을 막는다).
//! - Windows는 파일 이름의 대소문자를 구분하지 않으므로, 대소문자를 무시하고 찾는다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use st_core::{hash, AccountRef, Issue};

/// 한 폴더에서 읽는 항목 수 상한 (비정상적으로 큰 폴더 방어).
const MAX_DIR_ENTRIES: usize = 200_000;

const BROWSERS_JSON: &str = include_str!("../../../config/browsers.json");

pub const ARTIFACT_RECYCLE_BIN: &str = "recycle_bin";
pub const ARTIFACT_LNK: &str = "lnk";
pub const ARTIFACT_CHROMIUM: &str = "chromium_history";
pub const ARTIFACT_FIREFOX: &str = "firefox_history";
pub const ARTIFACT_HIVE_SYSTEM: &str = "hive_system";
pub const ARTIFACT_HIVE_SOFTWARE: &str = "hive_software";
pub const ARTIFACT_EVTX_SECURITY: &str = "evtx_security";
pub const ARTIFACT_EVTX_SYSTEM: &str = "evtx_system";
pub const ARTIFACT_EVTX_PARTITION: &str = "evtx_partition";
pub const ARTIFACT_EVTX_WLAN: &str = "evtx_wlan";
pub const ARTIFACT_EVTX_RDP_SESSION: &str = "evtx_rdp_session";
pub const ARTIFACT_EVTX_RDP_AUTH: &str = "evtx_rdp_auth";
pub const ARTIFACT_HIVE_AMCACHE: &str = "hive_amcache";
pub const ARTIFACT_SRUM: &str = "srum";
pub const ARTIFACT_PREFETCH: &str = "prefetch";
pub const ARTIFACT_JUMPLIST: &str = "jumplist";
pub const ARTIFACT_HIVE_NTUSER: &str = "hive_ntuser";
pub const ARTIFACT_HIVE_USRCLASS: &str = "hive_usrclass";

/// 찾으려는 아티팩트 전체 목록 (못 찾은 것도 보고서에 "없음"으로 나와야 한다).
pub const ALL_ARTIFACTS: [&str; 18] = [
    ARTIFACT_HIVE_SYSTEM,
    ARTIFACT_HIVE_SOFTWARE,
    ARTIFACT_HIVE_NTUSER,
    ARTIFACT_HIVE_USRCLASS,
    ARTIFACT_HIVE_AMCACHE,
    ARTIFACT_EVTX_SECURITY,
    ARTIFACT_EVTX_SYSTEM,
    ARTIFACT_EVTX_PARTITION,
    ARTIFACT_EVTX_WLAN,
    ARTIFACT_EVTX_RDP_SESSION,
    ARTIFACT_EVTX_RDP_AUTH,
    ARTIFACT_PREFETCH,
    ARTIFACT_SRUM,
    ARTIFACT_RECYCLE_BIN,
    ARTIFACT_LNK,
    ARTIFACT_JUMPLIST,
    ARTIFACT_CHROMIUM,
    ARTIFACT_FIREFOX,
];

/// 사용자 폴더 기준 경로 — 사용자 레지스트리 하이브. 실행 중인 PC에서는 로그온한 사용자의 것이
/// 잠겨 있어 수집 단계가 `reg save`로 사본을 만든다 (같은 폴더 구조로 overlay에).
pub const USER_HIVES: [(&str, &[&str]); 2] = [
    (ARTIFACT_HIVE_NTUSER, &["NTUSER.DAT"]),
    (ARTIFACT_HIVE_USRCLASS, &["AppData", "Local", "Microsoft", "Windows", "UsrClass.dat"]),
];

/// (아티팩트, Windows 폴더 기준 경로) — 레지스트리 하이브와 이벤트 로그.
/// 실행 중인 PC에서는 잠겨 있어 수집 단계(live)가 사본을 만들어 둔다.
pub const SYSTEM_FILES: [(&str, &[&str]); 10] = [
    (ARTIFACT_HIVE_SYSTEM, &["Windows", "System32", "config", "SYSTEM"]),
    (ARTIFACT_HIVE_SOFTWARE, &["Windows", "System32", "config", "SOFTWARE"]),
    (ARTIFACT_EVTX_SECURITY, &["Windows", "System32", "winevt", "Logs", "Security.evtx"]),
    (ARTIFACT_EVTX_SYSTEM, &["Windows", "System32", "winevt", "Logs", "System.evtx"]),
    (
        ARTIFACT_EVTX_PARTITION,
        &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-Partition%4Diagnostic.evtx"],
    ),
    (
        ARTIFACT_EVTX_WLAN,
        &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-WLAN-AutoConfig%4Operational.evtx"],
    ),
    (
        ARTIFACT_EVTX_RDP_SESSION,
        &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-TerminalServices-LocalSessionManager%4Operational.evtx"],
    ),
    (
        ARTIFACT_EVTX_RDP_AUTH,
        &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-TerminalServices-RemoteConnectionManager%4Operational.evtx"],
    ),
    (ARTIFACT_HIVE_AMCACHE, &["Windows", "appcompat", "Programs", "Amcache.hve"]),
    (ARTIFACT_SRUM, &["Windows", "System32", "sru", "SRUDB.dat"]),
];

#[derive(Debug, Clone, Deserialize)]
struct BrowserDef {
    name: String,
    family: String,
    path_under_user: Vec<String>,
}

/// 입력에서 찾은 파일 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundItem {
    pub artifact: String,
    /// 입력 루트 기준 상대 경로 ('/' 구분). 보고서와 근거 파일에는 이 값만 쓴다.
    pub rel_path: String,
    #[serde(skip)]
    pub abs_path: PathBuf,
    pub account: AccountRef,
    /// 브라우저 이름과 프로필 (브라우저 기록일 때만)
    pub browser: Option<String>,
    pub profile: Option<String>,
}

/// 아티팩트별로 몇 개를 찾았는지.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCheck {
    pub artifact: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    #[serde(skip)]
    pub root: PathBuf,
    /// 수집 사본 폴더 (실행 중 PC 점검일 때). 하이브·이벤트 로그는 여기서 먼저 찾는다.
    #[serde(skip)]
    pub overlay: Option<PathBuf>,
    pub items: Vec<FoundItem>,
    pub checks: Vec<ScanCheck>,
    pub issues: Vec<Issue>,
}

#[derive(Debug)]
pub enum ScanError {
    InputNotFound,
}

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// 폴더 내용을 이름순으로 돌려준다. 심볼릭 링크는 뺀다. 읽지 못하면 빈 목록.
fn list_dir(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten().take(MAX_DIR_ENTRIES) {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        out.push(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            is_dir: ft.is_dir(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 대소문자를 무시하고 하위 항목 하나를 찾는다.
fn child_ci(dir: &Path, name: &str) -> Option<Entry> {
    list_dir(dir).into_iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// 대소문자를 무시하고 여러 단계를 내려간다.
fn descend_ci(start: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    for part in parts {
        let next = child_ci(&cur, part)?;
        if !next.is_dir {
            return None;
        }
        cur = next.path;
    }
    Some(cur)
}

fn rel(root: &Path, path: &Path) -> String {
    let p = path.strip_prefix(root).unwrap_or(path);
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// 입력 폴더를 훑어 아티팩트 파일 목록을 만든다.
pub fn scan(root: &Path) -> Result<Scan, ScanError> {
    scan_with_overlay(root, None)
}

/// `overlay`가 있으면 레지스트리 하이브·이벤트 로그는 그 폴더(같은 폴더 구조)에서 먼저 찾는다.
pub fn scan_with_overlay(root: &Path, overlay: Option<&Path>) -> Result<Scan, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::InputNotFound);
    }
    let mut items: Vec<FoundItem> = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();

    // --- 레지스트리 하이브, 이벤트 로그 ---
    for (artifact, parts) in SYSTEM_FILES {
        let (dir_parts, file) = match parts.split_last() {
            Some((f, d)) => (d, *f),
            None => continue,
        };
        let mut found = None;
        for base in overlay.into_iter().chain(std::iter::once(root)) {
            if let Some(dir) = descend_ci(base, dir_parts) {
                if let Some(f) = child_ci(&dir, file).filter(|e| !e.is_dir) {
                    found = Some((base.to_path_buf(), f.path));
                    break;
                }
            }
        }
        if let Some((base, path)) = found {
            let from_overlay = overlay.is_some_and(|o| o == base);
            items.push(FoundItem {
                artifact: artifact.to_string(),
                rel_path: if from_overlay {
                    format!("[수집본]/{}", rel(&base, &path))
                } else {
                    rel(root, &path)
                },
                abs_path: path,
                account: AccountRef::default(),
                browser: None,
                profile: None,
            });
        }
    }

    // --- 휴지통: $Recycle.Bin\<SID>\$I* ---
    if let Some(bin) = descend_ci(root, &["$Recycle.Bin"]) {
        for sid_dir in list_dir(&bin).into_iter().filter(|e| e.is_dir) {
            for f in list_dir(&sid_dir.path) {
                let upper = f.name.to_ascii_uppercase();
                if !f.is_dir && upper.starts_with("$I") {
                    items.push(FoundItem {
                        artifact: ARTIFACT_RECYCLE_BIN.to_string(),
                        rel_path: rel(root, &f.path),
                        abs_path: f.path,
                        account: AccountRef { sid: Some(sid_dir.name.clone()), name: None },
                        browser: None,
                        profile: None,
                    });
                }
            }
        }
    }

    // --- Prefetch: Windows\Prefetch\*.pf ---
    if let Some(dir) = descend_ci(root, &["Windows", "Prefetch"]) {
        for f in list_dir(&dir) {
            if !f.is_dir && f.name.to_ascii_lowercase().ends_with(".pf") {
                items.push(FoundItem {
                    artifact: ARTIFACT_PREFETCH.to_string(),
                    rel_path: rel(root, &f.path),
                    abs_path: f.path,
                    account: AccountRef::default(),
                    browser: None,
                    profile: None,
                });
            }
        }
    }

    // --- 사용자별 자료 ---
    let browsers: Vec<BrowserDef> = match serde_json::from_str(BROWSERS_JSON) {
        Ok(b) => b,
        Err(e) => {
            issues.push(Issue {
                artifact: "config".to_string(),
                file: "config/browsers.json".to_string(),
                reason: format!("브라우저 목록을 읽지 못함: {e}"),
            });
            Vec::new()
        }
    };

    if let Some(users) = descend_ci(root, &["Users"]) {
        for user in list_dir(&users).into_iter().filter(|e| e.is_dir) {
            let account = AccountRef { sid: None, name: Some(user.name.clone()) };

            // 사용자 레지스트리 하이브 (수집본이 있으면 그쪽)
            for (artifact, parts) in USER_HIVES {
                let (dir_parts, file) = match parts.split_last() {
                    Some((f, d)) => (d, *f),
                    None => continue,
                };
                let mut found: Option<(PathBuf, PathBuf)> = None;
                let over_user = overlay.map(|o| o.join("Users").join(&user.name));
                for base in over_user.iter().map(PathBuf::as_path).chain(std::iter::once(user.path.as_path())) {
                    let dir = if dir_parts.is_empty() { Some(base.to_path_buf()) } else { descend_ci(base, dir_parts) };
                    if let Some(f) = dir.and_then(|d| child_ci(&d, file)).filter(|e| !e.is_dir) {
                        found = Some((base.to_path_buf(), f.path));
                        break;
                    }
                }
                if let Some((base, path)) = found {
                    let from_overlay = over_user.as_deref() == Some(base.as_path());
                    let rel_path = match overlay {
                        Some(o) if from_overlay => format!("[수집본]/{}", rel(o, &path)),
                        _ => rel(root, &path),
                    };
                    items.push(FoundItem {
                        artifact: artifact.to_string(),
                        rel_path,
                        abs_path: path,
                        account: account.clone(),
                        browser: None,
                        profile: None,
                    });
                }
            }

            // 점프 목록
            for (sub, ext) in [("AutomaticDestinations", ".automaticdestinations-ms"), ("CustomDestinations", ".customdestinations-ms")] {
                let Some(dir) = descend_ci(&user.path, &["AppData", "Roaming", "Microsoft", "Windows", "Recent", sub]) else {
                    continue;
                };
                for f in list_dir(&dir) {
                    if !f.is_dir && f.name.to_ascii_lowercase().ends_with(ext) {
                        items.push(FoundItem {
                            artifact: ARTIFACT_JUMPLIST.to_string(),
                            rel_path: rel(root, &f.path),
                            abs_path: f.path,
                            account: account.clone(),
                            browser: None,
                            profile: None,
                        });
                    }
                }
            }

            // 최근 연 파일: Windows Recent 폴더와 Office Recent 폴더의 *.lnk
            for recent in [
                ["AppData", "Roaming", "Microsoft", "Windows", "Recent"],
                ["AppData", "Roaming", "Microsoft", "Office", "Recent"],
            ] {
                let Some(dir) = descend_ci(&user.path, &recent) else { continue };
                for f in list_dir(&dir) {
                    if !f.is_dir && f.name.to_ascii_lowercase().ends_with(".lnk") {
                        items.push(FoundItem {
                            artifact: ARTIFACT_LNK.to_string(),
                            rel_path: rel(root, &f.path),
                            abs_path: f.path,
                            account: account.clone(),
                            browser: None,
                            profile: None,
                        });
                    }
                }
            }

            // 브라우저 기록
            for b in &browsers {
                let parts: Vec<&str> = b.path_under_user.iter().map(String::as_str).collect();
                let Some(base) = descend_ci(&user.path, &parts) else { continue };
                let (artifact, db_name) = match b.family.as_str() {
                    "chromium" => (ARTIFACT_CHROMIUM, "History"),
                    "firefox" => (ARTIFACT_FIREFOX, "places.sqlite"),
                    _ => continue,
                };
                // 프로필 폴더 이름은 정해져 있지 않으므로, DB 파일이 들어 있는 하위 폴더를 모두 본다.
                for profile in list_dir(&base).into_iter().filter(|e| e.is_dir) {
                    if let Some(db) = child_ci(&profile.path, db_name).filter(|e| !e.is_dir) {
                        items.push(FoundItem {
                            artifact: artifact.to_string(),
                            rel_path: rel(root, &db.path),
                            abs_path: db.path,
                            account: account.clone(),
                            browser: Some(b.name.clone()),
                            profile: Some(profile.name.clone()),
                        });
                    }
                }
            }
        }
    }

    items.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    let checks = ALL_ARTIFACTS
        .iter()
        .map(|a| ScanCheck {
            artifact: (*a).to_string(),
            count: items.iter().filter(|i| i.artifact == *a).count(),
        })
        .collect();

    Ok(Scan { root: root.to_path_buf(), overlay: overlay.map(Path::to_path_buf), items, checks, issues })
}

/// 하이브 옆의 트랜잭션 로그 (.LOG1, .LOG2, 대소문자 무시).
pub fn hive_logs(hive: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (hive.parent(), hive.file_name()) else { return Vec::new() };
    let name = name.to_string_lossy().to_ascii_lowercase();
    let mut v: Vec<PathBuf> = list_dir(dir)
        .into_iter()
        .filter(|e| !e.is_dir)
        .filter(|e| {
            let n = e.name.to_ascii_lowercase();
            n == format!("{name}.log1") || n == format!("{name}.log2")
        })
        .map(|e| e.path)
        .collect();
    v.sort();
    v
}

/// SQLite DB 옆에 있을 수 있는 보조 파일 이름들.
pub fn sqlite_sidecars(db: &Path) -> Vec<PathBuf> {
    let Some(name) = db.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    ["-wal", "-journal", "-shm"]
        .iter()
        .map(|suffix| db.with_file_name(format!("{name}{suffix}")))
        .filter(|p| p.is_file())
        .collect()
}

/// 찾은 파일 전부(브라우저 DB의 보조 파일 포함)의 SHA-256. 키는 상대 경로.
/// 분석 전과 후에 한 번씩 불러 두 결과를 비교하면 입력이 바뀌지 않았음을 보일 수 있다.
pub fn hash_inputs(scan: &Scan) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for item in &scan.items {
        let mut paths = vec![item.abs_path.clone()];
        if item.artifact == ARTIFACT_CHROMIUM || item.artifact == ARTIFACT_FIREFOX {
            paths.extend(sqlite_sidecars(&item.abs_path));
        }
        if item.artifact.starts_with("hive_") {
            paths.extend(hive_logs(&item.abs_path));
        }
        for p in paths {
            let value = hash::sha256_file(&p).unwrap_or_else(|e| format!("읽기 실패: {e}"));
            let key = match &scan.overlay {
                Some(o) if p.starts_with(o) => format!("[수집본]/{}", rel(o, &p)),
                _ => rel(&scan.root, &p),
            };
            out.insert(key, value);
        }
    }
    out
}

/// `child`가 `parent` 안에 있는가 (같은 경로 포함). 출력 폴더가 입력 안에 있는지 확인하는 데 쓴다.
/// 아직 없는 경로는 존재하는 가장 가까운 상위 폴더를 기준으로 판단한다.
pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let Ok(parent) = parent.canonicalize() else {
        return false;
    };
    let mut probe = child.to_path_buf();
    loop {
        if let Ok(c) = probe.canonicalize() {
            return c.starts_with(&parent);
        }
        if !probe.pop() {
            return false;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-collect-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn finds_artifacts_ignoring_case() {
        let root = temp_root("scan");
        touch(&root.join("$RECYCLE.BIN/S-1-5-21-1-2-3-1001/$IABCDEF.txt"));
        touch(&root.join("$RECYCLE.BIN/S-1-5-21-1-2-3-1001/$RABCDEF.txt"));
        // 대소문자가 다른 경로. (Linux에서는 "AppData"와 "appdata"가 따로 생길 수 있으므로
        // 한 사용자 안에서는 표기를 섞지 않는다. Windows에서는 애초에 같은 폴더다.)
        touch(&root.join("users/park/appdata/roaming/microsoft/windows/recent/보고서.lnk"));
        touch(&root.join("users/park/appdata/roaming/microsoft/windows/recent/notes.txt"));
        touch(&root.join("users/Kim/AppData/Local/Google/Chrome/User Data/Default/History"));
        touch(&root.join("users/Kim/AppData/Local/Google/Chrome/User Data/Default/History-wal"));
        touch(&root.join("users/Kim/AppData/Local/Google/Chrome/User Data/Profile 1/History"));
        touch(&root.join("users/Lee/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/places.sqlite"));

        touch(&root.join("Windows/Prefetch/NOTEPAD.EXE-D8414F97.pf"));
        touch(&root.join("Windows/Prefetch/Layout.ini"));
        touch(&root.join("users/Kim/NTUSER.DAT"));
        touch(&root.join("users/Kim/AppData/Local/Microsoft/Windows/UsrClass.dat"));
        touch(&root.join("users/Kim/AppData/Roaming/Microsoft/Windows/Recent/AutomaticDestinations/9b9cdc69c1c24e2b.automaticDestinations-ms"));
        touch(&root.join("users/Kim/AppData/Roaming/Microsoft/Windows/Recent/CustomDestinations/5afe4de1b92fc382.customDestinations-ms"));

        let s = scan(&root).unwrap();
        let count = |a: &str| s.checks.iter().find(|c| c.artifact == a).unwrap().count;
        assert_eq!(count(ARTIFACT_PREFETCH), 1, "Layout.ini는 세지 않는다");
        assert_eq!(count(ARTIFACT_HIVE_NTUSER), 1);
        assert_eq!(count(ARTIFACT_HIVE_USRCLASS), 1);
        assert_eq!(count(ARTIFACT_JUMPLIST), 2);
        assert_eq!(count(ARTIFACT_RECYCLE_BIN), 1, "$R 파일은 세지 않는다");
        assert_eq!(count(ARTIFACT_LNK), 1);
        assert_eq!(count(ARTIFACT_CHROMIUM), 2);
        assert_eq!(count(ARTIFACT_FIREFOX), 1);

        let bin = s.items.iter().find(|i| i.artifact == ARTIFACT_RECYCLE_BIN).unwrap();
        assert_eq!(bin.account.sid.as_deref(), Some("S-1-5-21-1-2-3-1001"));
        let chrome = s.items.iter().find(|i| i.profile.as_deref() == Some("Profile 1")).unwrap();
        assert_eq!(chrome.account.name.as_deref(), Some("Kim"));
        assert_eq!(chrome.browser.as_deref(), Some("Chrome"));

        // 하이브: 수집본(overlay)에 있으면 그쪽을 쓴다.
        touch(&root.join("Windows/System32/config/SYSTEM"));
        let ov = temp_root("overlay");
        touch(&ov.join("Windows/System32/config/SYSTEM"));
        touch(&ov.join("Windows/System32/config/SYSTEM.LOG1"));
        let s2 = scan_with_overlay(&root, Some(&ov)).unwrap();
        let hive = s2.items.iter().find(|i| i.artifact == ARTIFACT_HIVE_SYSTEM).unwrap();
        assert!(hive.abs_path.starts_with(&ov));
        assert!(hive.rel_path.starts_with("[수집본]/"));
        assert!(hash_inputs(&s2).keys().any(|k| k.ends_with("SYSTEM.LOG1")));
        // 사용자 하이브도 수집본을 먼저 쓴다.
        touch(&ov.join("Users/Kim/NTUSER.DAT"));
        let s3 = scan_with_overlay(&root, Some(&ov)).unwrap();
        let nt = s3.items.iter().find(|i| i.artifact == ARTIFACT_HIVE_NTUSER).unwrap();
        assert!(nt.abs_path.starts_with(&ov), "{:?}", nt.abs_path);
        assert_eq!(nt.rel_path, "[수집본]/Users/Kim/NTUSER.DAT");
        assert_eq!(nt.account.name.as_deref(), Some("Kim"));

        // 해시에는 보조 파일(-wal)도 포함된다.
        let hashes = hash_inputs(&s);
        assert!(hashes.keys().any(|k| k.ends_with("History-wal")));
        assert_eq!(hashes.len(), 11);
    }

    #[test]
    fn empty_input_reports_everything_missing() {
        let root = temp_root("empty");
        let s = scan(&root).unwrap();
        assert_eq!(s.checks.len(), ALL_ARTIFACTS.len());
        assert_eq!(s.checks.len(), 18);
        assert!(s.checks.iter().all(|c| c.count == 0));
        assert!(matches!(scan(&root.join("nope")), Err(ScanError::InputNotFound)));
    }

    #[test]
    fn detects_output_inside_input() {
        let root = temp_root("inside");
        fs::create_dir_all(root.join("a/b")).unwrap();
        assert!(is_inside(&root.join("a/b"), &root));
        assert!(is_inside(&root.join("a/new/out"), &root), "아직 없는 경로도 판단한다");
        assert!(is_inside(&root, &root));
        assert!(!is_inside(&std::env::temp_dir(), &root));
    }
}
