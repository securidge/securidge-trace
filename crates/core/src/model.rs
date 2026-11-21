//! 공통 데이터 모델.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 원본 시각 값이 어떤 기준으로 저장돼 있었는지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TimeBasis {
    /// 원본이 UTC였다.
    Utc,
    /// 원본이 대상 PC의 로컬 시간이었다 (UTC로 바꾸려면 대상 PC의 시간대 설정이 필요).
    Local,
    /// 파일 시스템의 수정 시각처럼, 수집 과정에서 바뀌었을 수 있는 값.
    FileSystem,
}

/// 시각 하나. UTC 값과 원본 값을 함께 보관한다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    /// UTC로 변환한 값. 변환할 수 없으면 None.
    pub utc: Option<DateTime<Utc>>,
    /// 원본에 적혀 있던 값 (사람이 다시 확인할 수 있도록 그대로 남긴다).
    pub raw: String,
    pub basis: TimeBasis,
}

impl Stamp {
    pub fn utc(dt: Option<DateTime<Utc>>, raw: impl Into<String>) -> Self {
        Self { utc: dt, raw: raw.into(), basis: TimeBasis::Utc }
    }
    pub fn file_system(dt: Option<DateTime<Utc>>, raw: impl Into<String>) -> Self {
        Self { utc: dt, raw: raw.into(), basis: TimeBasis::FileSystem }
    }
}

/// 사실의 신뢰도.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// 사실의 종류. 단계가 진행되면 늘어난다 (장치 연결, 네트워크 연결 등).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventKind {
    /// 파일이 휴지통으로 이동됨
    FileDeleted,
    /// 파일이 열림 (LNK 근거. "복사했다"는 뜻이 아니다)
    FileOpened,
    /// 웹 페이지 방문
    WebVisit,
    /// 웹에서 파일 내려받음
    WebDownload,
    /// 장치가 이 PC에 처음 설치(연결)됨
    DeviceFirstConnected,
    /// 장치가 마지막으로 연결됨
    DeviceLastConnected,
    /// 장치가 마지막으로 분리됨
    DeviceLastRemoved,
    /// 저장장치 연결·분리 기록 (이벤트 로그, 볼륨 시리얼 포함 가능)
    StorageAttached,
    /// 네트워크에 처음 연결됨
    NetworkFirstConnected,
    /// 네트워크에 마지막으로 연결됨
    NetworkLastConnected,
    /// 무선 네트워크 연결 (이벤트 로그)
    WlanConnected,
    /// 무선 네트워크 연결 끊김 (이벤트 로그)
    WlanDisconnected,
    /// 블루투스 장치 마지막 연결
    BluetoothConnected,
    /// 이벤트 로그가 지워짐
    LogCleared,
    /// 시스템 시각이 바뀜
    TimeChanged,
    /// 프로그램 실행 (Prefetch, BAM, UserAssist)
    ProgramRun,
    /// 실행 파일이 PC에 있었음 (Amcache. 실행했다는 뜻은 아님)
    ProgramPresent,
    /// 탐색기로 폴더를 열어 봄 (ShellBags)
    FolderOpened,
    /// 원격 데스크톱 접속 (로그온·인증·연결 끊김·다시 연결)
    RemoteLogon,
    /// 프로그램별 하루 네트워크 전송량 (SRUM)
    NetworkUsage,
}

/// 이 사실이 어디에서 나왔는지.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceRef {
    /// 아티팩트 종류 (예: "recycle_bin", "lnk", "chromium_history")
    pub artifact: String,
    /// 입력 루트 기준 상대 경로
    pub file: String,
    /// 파일 안에서의 위치 (예: "visits.id=12"). 파일 전체가 한 레코드면 빈 문자열.
    pub record: String,
}

/// Windows 계정. SID와 계정명 중 아는 것만 채운다.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountRef {
    pub sid: Option<String>,
    pub name: Option<String>,
}

/// 파서가 뽑아낸 "사실" 하나.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Event {
    pub time: Stamp,
    pub kind: EventKind,
    /// 대상 (파일 경로 또는 URL)
    pub subject: String,
    pub account: AccountRef,
    /// 종류별 부가 정보. BTreeMap을 써서 출력 순서가 항상 같다 (재현 가능성).
    pub details: BTreeMap<String, String>,
    pub source: SourceRef,
    pub confidence: Confidence,
    /// 이 사실을 읽을 때 주의할 점 (예: "수집 과정에서 시각이 바뀌었을 수 있음")
    pub caveats: Vec<String>,
}

/// 읽지 못했거나 건너뛴 항목. 프로그램을 멈추지 않고 여기에 쌓는다.
/// reason이 "참고:"로 시작하면 실패가 아니라 알아 둘 내용이다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Issue {
    pub artifact: String,
    pub file: String,
    pub reason: String,
}

impl Issue {
    pub fn is_note(&self) -> bool {
        self.reason.starts_with("참고:")
    }
}

/// 점검 시점에 PC에 연결돼 있던 드라이브 (실행 중인 PC 점검에서만 채운다).
/// 장치 경로(`\Device\HarddiskVolume3`)와 볼륨 일련번호를 드라이브 문자로 바꾸는 데 쓴다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveInfo {
    /// `C:`
    pub letter: String,
    /// `\Device\HarddiskVolume3`
    pub device: String,
    /// `1A2B-3C4D`
    pub serial: Option<String>,
    pub removable: bool,
}

/// 아티팩트별 데이터 범위 (개발 요청서 2장 7번: "기록 없음"과 "행위 없음"을 구분).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub artifact: String,
    /// 찾은 파일 수
    pub files: usize,
    /// 뽑아낸 사실 수
    pub events: usize,
    pub earliest: Option<DateTime<Utc>>,
    pub latest: Option<DateTime<Utc>>,
}
