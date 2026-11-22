//! 설정과 고객사 프로필. 화면(GUI)과 명령줄이 같은 구조를 쓴다.
//!
//! 모든 값에 기본값이 있어, 아무것도 설정하지 않아도 점검이 된다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 화면 언어: "ko" 또는 "en". 비어 있으면 첫 실행.
    pub ui_lang: String,
    /// 보고서 기본 언어
    pub report_lang: String,
    /// 표시 시간대 (IANA 이름)
    pub time_zone: String,
    /// 점검 업체명 (보고서 표지에 표시)
    pub company_name: String,
    /// 기본 점검자 이름
    pub analyst: String,
    /// 기본 점검 기간(일). 0이면 전체.
    pub default_period_days: u32,
    /// 첫 실행 안내에 동의했는가
    pub consent_accepted: bool,
    /// 마지막으로 고른 고객사
    pub last_customer: String,
    /// 전체 브라우저 방문 기록을 별도 파일로 내보낼지 (기본 꺼짐)
    pub export_full_browser_history: bool,
    /// 보고서에서 계정명·파일명을 가릴지 (교육·시연용)
    pub mask_names: bool,
    /// 수집본(레지스트리·이벤트 로그 사본)을 내역에 남길지 (기본 꺼짐)
    pub keep_collected_copies: bool,
    /// 글자 크기 배율 (1.0 = 기본)
    pub ui_scale: f32,
    /// 화면 테마: "dark" 또는 "light"
    pub theme: String,
    pub rules: RuleConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ui_lang: String::new(),
            report_lang: "ko".to_string(),
            time_zone: "Asia/Seoul".to_string(),
            company_name: String::new(),
            analyst: String::new(),
            default_period_days: 30,
            consent_accepted: false,
            last_customer: String::new(),
            export_full_browser_history: false,
            mask_names: false,
            keep_collected_copies: false,
            ui_scale: 1.0,
            theme: "dark".to_string(),
            rules: RuleConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleConfig {
    pub new_storage_device: bool,
    pub after_hours_storage_use: bool,
    pub files_opened_from_removable: bool,
    pub mass_delete: bool,
    /// 이 시간(분) 안에 휴지통으로 옮긴 파일이 `mass_delete_count`개 이상이면 '대량 삭제'
    pub mass_delete_window_minutes: u32,
    pub mass_delete_count: u32,
    pub log_cleared: bool,
    pub time_changed: bool,
    /// 사람 계정이 이 시간(분) 이상 시각을 바꾸면 '확인 필요'
    pub time_change_minutes: u32,
    pub portable_device: bool,
    pub cloud_or_webmail: bool,
    /// 클라우드·웹메일 접속이 이 횟수 이상이면 '주의'
    pub cloud_visit_count: u32,
    /// 주소 분류표: 클라우드 저장소
    pub cloud_domains: Vec<String>,
    /// 주소 분류표: 웹메일
    pub webmail_domains: Vec<String>,
    /// 주소 분류표: 파일 공유·전송
    pub file_share_domains: Vec<String>,
    /// R9 이동식 저장장치에서 프로그램 실행
    pub program_from_removable: bool,
    /// R10 흔적 지우기·완전 삭제 프로그램 실행
    pub cleanup_tools: bool,
    pub cleanup_tool_names: Vec<String>,
    /// R11 원격 데스크톱 접속
    pub remote_logon: bool,
    /// R12 하루 전송량이 이 값(MB) 이상인 프로그램
    pub large_upload: bool,
    pub large_upload_mb: u32,
    /// R13 원격 제어 프로그램 실행
    pub remote_tools: bool,
    pub remote_tool_names: Vec<String>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        let v = |xs: &[&str]| xs.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        RuleConfig {
            new_storage_device: true,
            after_hours_storage_use: true,
            files_opened_from_removable: true,
            mass_delete: true,
            mass_delete_window_minutes: 30,
            mass_delete_count: 20,
            log_cleared: true,
            time_changed: true,
            time_change_minutes: 10,
            portable_device: true,
            cloud_or_webmail: true,
            cloud_visit_count: 10,
            cloud_domains: v(&[
                "drive.google.com",
                "docs.google.com",
                "onedrive.live.com",
                "1drv.ms",
                "sharepoint.com",
                "dropbox.com",
                "box.com",
                "icloud.com",
                "mybox.naver.com",
                "drive.daum.net",
                "mega.nz",
            ]),
            webmail_domains: v(&[
                "mail.google.com",
                "outlook.live.com",
                "outlook.office.com",
                "mail.naver.com",
                "mail.daum.net",
                "mail.nate.com",
                "mail.yahoo.com",
            ]),
            file_share_domains: v(&[
                "wetransfer.com",
                "sendanywhere.com",
                "send-anywhere.com",
                "transfer.sh",
                "filemail.com",
                "gofile.io",
            ]),
            program_from_removable: true,
            cleanup_tools: true,
            cleanup_tool_names: v(&[
                "ccleaner.exe",
                "ccleaner64.exe",
                "bleachbit.exe",
                "bleachbit_console.exe",
                "eraser.exe",
                "eraserl.exe",
                "sdelete.exe",
                "sdelete64.exe",
                "privazer.exe",
                "hardwipe.exe",
                "fileshredder.exe",
            ]),
            remote_logon: true,
            large_upload: true,
            large_upload_mb: 500,
            remote_tools: true,
            remote_tool_names: v(&[
                "anydesk.exe",
                "teamviewer.exe",
                "teamviewer_service.exe",
                "rustdesk.exe",
                "remoting_host.exe",
                "rutserv.exe",
                "rfusclient.exe",
                "ammyy_admin.exe",
                "aa_v3.exe",
                "splashtopstreamer.exe",
                "strwinclt.exe",
                "logmein.exe",
                "screenconnect.windowsclient.exe",
                "parsecd.exe",
            ]),
        }
    }
}

/// 요일 비트: 월=1, 화=2, 수=4, 목=8, 금=16, 토=32, 일=64
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Customer {
    pub name: String,
    /// 업무 시작·종료 (시, 0~24)
    pub work_start_hour: u32,
    pub work_end_hour: u32,
    pub work_days_mask: u8,
    /// 회사 장치 목록 (장치 일련번호 또는 볼륨 시리얼)
    pub allowed_devices: Vec<AllowedDevice>,
}

impl Default for Customer {
    fn default() -> Self {
        Customer {
            name: String::new(),
            work_start_hour: 9,
            work_end_hour: 18,
            work_days_mask: 0b0001_1111,
            allowed_devices: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowedDevice {
    /// 장치 일련번호(USBSTOR 인스턴스) 또는 볼륨 시리얼(`1A2B-3C4D`)
    pub id: String,
    pub memo: String,
}

impl Customer {
    pub fn is_allowed(&self, id: &str) -> bool {
        let id = id.trim();
        !id.is_empty() && self.allowed_devices.iter().any(|d| d.id.trim().eq_ignore_ascii_case(id))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"ui_lang":"en"}"#).unwrap();
        assert_eq!(s.ui_lang, "en");
        assert_eq!(s.time_zone, "Asia/Seoul");
        assert_eq!(s.rules.mass_delete_count, 20);
        let c: Customer = serde_json::from_str(r#"{"name":"A"}"#).unwrap();
        assert_eq!(c.work_end_hour, 18);
        assert!(!c.is_allowed(""));
    }
}
