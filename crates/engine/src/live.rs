//! 실행 중인 Windows PC에서 잠긴 자료를 읽는다.
//!
//! 레지스트리 하이브와 이벤트 로그는 Windows가 쓰는 중이라 일반 복사가 안 된다.
//! Windows에 들어 있는 공식 도구로 사본을 만든다.
//! - 레지스트리: `reg save HKLM\SYSTEM <파일>` (관리자 권한 필요)
//! - 이벤트 로그: `wevtutil epl <로그 이름> <파일>`
//!
//! 대상 PC에 남는 변화: 사본은 작업 폴더(도구 옆 data 폴더)에만 만든다. `reg save`와
//! `wevtutil epl`은 대상 PC의 기록 자체를 바꾸지 않는다.
//! (결정 기록 D2는 "디스크 직접 읽기"였으나, v0.1은 검증된 Windows 기본 도구를 쓰는 쪽으로 바꿨다.
//!  docs/decisions.md 참고)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use st_report::CollectedFile;
use st_rules::ToolMedia;

/// (아티팩트, 사본 경로(Windows 폴더 구조), 명령, 인자)
fn plan() -> Vec<(&'static str, &'static [&'static str], &'static str, Vec<&'static str>)> {
    vec![
        ("hive_system", &["Windows", "System32", "config", "SYSTEM"], "reg", vec!["save", "HKLM\\SYSTEM"]),
        ("hive_software", &["Windows", "System32", "config", "SOFTWARE"], "reg", vec!["save", "HKLM\\SOFTWARE"]),
        ("evtx_security", &["Windows", "System32", "winevt", "Logs", "Security.evtx"], "wevtutil", vec!["epl", "Security"]),
        ("evtx_system", &["Windows", "System32", "winevt", "Logs", "System.evtx"], "wevtutil", vec!["epl", "System"]),
        (
            "evtx_partition",
            &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-Partition%4Diagnostic.evtx"],
            "wevtutil",
            vec!["epl", "Microsoft-Windows-Partition/Diagnostic"],
        ),
        (
            "evtx_wlan",
            &["Windows", "System32", "winevt", "Logs", "Microsoft-Windows-WLAN-AutoConfig%4Operational.evtx"],
            "wevtutil",
            vec!["epl", "Microsoft-Windows-WLAN-AutoConfig/Operational"],
        ),
    ]
}

fn command(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// 잠긴 자료의 사본을 `overlay` 아래에 Windows 폴더 구조로 만든다.
pub fn collect(overlay: &Path, progress: &dyn Fn(&str), cancel: &AtomicBool) -> Result<Vec<CollectedFile>, String> {
    let mut out = Vec::new();
    for (artifact, parts, program, args) in plan() {
        if cancel.load(Ordering::Relaxed) {
            return Err("취소했습니다.".to_string());
        }
        let mut dst: PathBuf = overlay.to_path_buf();
        for p in parts {
            dst.push(p);
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("작업 폴더를 만들지 못함: {e}"))?;
        }
        let method = format!("{program} {}", args.join(" "));
        progress(&method);
        let mut cmd = command(program);
        cmd.args(&args).arg(&dst);
        if program == "reg" {
            cmd.arg("/y");
        } else {
            cmd.arg("/ow:true");
        }
        let res = cmd.output();
        let (ok, message) = match res {
            Ok(o) if o.status.success() && dst.is_file() => (true, String::new()),
            Ok(o) => (false, decode(&o.stderr).trim().to_string() + decode(&o.stdout).trim()),
            Err(e) => (false, e.to_string()),
        };
        let sha256 = if ok { st_core::hash::sha256_file(&dst).unwrap_or_default() } else { String::new() };
        out.push(CollectedFile { artifact: artifact.to_string(), method, ok, sha256, message });
    }
    Ok(out)
}

/// Windows 명령의 출력은 콘솔 코드 페이지(한국어 949)일 수 있다.
fn decode(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::EUC_KR.decode(b).0.into_owned(),
    }
}

/// 관리자 권한인가. (`net session`은 관리자에게만 성공한다.)
pub fn is_admin() -> bool {
    if !cfg!(windows) {
        return false;
    }
    command("net").arg("session").output().map(|o| o.status.success()).unwrap_or(false)
}

/// 시스템 드라이브 루트 (보통 C:\).
pub fn system_root() -> PathBuf {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    PathBuf::from(format!("{}\\", drive.trim_end_matches('\\')))
}

/// 점검 도구가 들어 있는 드라이브와 볼륨 일련번호. 시스템 드라이브면 비워 둔다.
pub fn tool_media(system_root: &Path) -> ToolMedia {
    let Ok(exe) = std::env::current_exe() else { return ToolMedia::default() };
    let exe = exe.display().to_string();
    let letter: String = exe.chars().take(2).collect();
    if letter.len() != 2 || !letter.ends_with(':') {
        return ToolMedia::default();
    }
    let sys: String = system_root.display().to_string().chars().take(2).collect();
    if letter.eq_ignore_ascii_case(&sys) {
        return ToolMedia::default();
    }
    let vsn = command("cmd")
        .args(["/c", "vol", &letter])
        .output()
        .ok()
        .and_then(|o| find_serial(&decode(&o.stdout)));
    ToolMedia { drive_letter: Some(letter.to_ascii_uppercase()), volume_serial: vsn }
}

/// `vol` 출력에서 `1A2B-3C4D` 꼴을 찾는다 (언어와 무관).
pub fn find_serial(text: &str) -> Option<String> {
    for word in text.split(|c: char| c.is_whitespace()) {
        let w = word.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != '-');
        if w.len() == 9 && w.as_bytes().get(4) == Some(&b'-') && w.chars().filter(|c| *c != '-').all(|c| c.is_ascii_hexdigit()) {
            return Some(w.to_ascii_uppercase());
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn serial_from_vol_output() {
        assert_eq!(find_serial(" Volume in drive E is STTEST\n Volume Serial Number is 1a2b-3C4D\n").as_deref(), Some("1A2B-3C4D"));
        assert_eq!(find_serial(" E 드라이브의 볼륨: STTEST\n 볼륨 일련 번호: E27C-9AED\n").as_deref(), Some("E27C-9AED"));
        assert_eq!(find_serial("no serial here 12-34"), None);
    }
}
