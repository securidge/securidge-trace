//! 바로 가기(.lnk) 파일 파서.
//!
//! Windows는 사용자가 파일을 열 때 `Recent` 폴더에 바로 가기를 만든다. 이 파일에는
//! 대상 파일의 경로뿐 아니라 **대상이 있던 볼륨의 종류(이동식인지)와 볼륨 시리얼 번호**가
//! 들어 있어, "어느 USB에 있던 파일을 열었는가"를 잇는 근거가 된다.
//!
//! 중요한 한계: 바로 가기는 "파일을 열었다"는 흔적이지 "복사했다"는 증거가 아니다.
//!
//! 이 파서가 읽는 부분 (필요한 최소한)
//! - ShellLinkHeader: 대상 파일의 생성·접근·수정 시각, 크기
//! - LinkInfo: VolumeID(드라이브 종류, 볼륨 시리얼, 볼륨 이름), 로컬 경로, 네트워크 공유 이름
//!
//! 아직 읽지 않는 부분 (Phase 2 과제)
//! - LinkTargetIDList의 셸 항목, StringData, ExtraData(TrackerDataBlock의 컴퓨터 이름 등)
//!
//! 근거: Microsoft 공개 규격 [MS-SHLLINK] "Shell Link (.LNK) Binary File Format"
//!   2.1 ShellLinkHeader, 2.2 LinkTargetIDList, 2.3 LinkInfo, 2.3.1 VolumeID,
//!   2.3.2 CommonNetworkRelativeLink
//!
//! 검증 상태: 실제 Windows 7 / 10 / Server 2019가 만든 샘플 17개로 테스트하고,
//! 독립적으로 만들어진 다른 파서(lnk_parser crate)의 결과와 비교함 (tests/lnk_samples.rs).
//! **미검증: Windows 11 샘플, 이동식 드라이브(USB)에 있던 파일의 바로 가기, 한글 경로,
//! 코드 페이지 949로만 저장된(유니코드 경로가 없는) 바로 가기.**

use chrono::{DateTime, Utc};
use encoding_rs::Encoding;
use serde::Serialize;
use st_core::bytes;
use st_core::time::filetime_to_utc;

const HEADER_SIZE: usize = 0x4C;
/// [MS-SHLLINK] 2.1 LinkCLSID: 00021401-0000-0000-C000-000000000046
const LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

const FLAG_HAS_LINK_TARGET_ID_LIST: u32 = 0x0000_0001;
const FLAG_HAS_LINK_INFO: u32 = 0x0000_0002;

const LINKINFO_VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;
const LINKINFO_COMMON_NETWORK_RELATIVE_LINK: u32 = 0x2;

/// 문자열 하나의 최대 길이(바이트). 조작된 파일이 메모리를 과하게 쓰지 못하게 한다.
const MAX_STRING_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LnkInfo {
    pub link_flags: u32,
    pub target_created_raw: u64,
    pub target_accessed_raw: u64,
    pub target_modified_raw: u64,
    pub target_created: Option<DateTime<Utc>>,
    pub target_accessed: Option<DateTime<Utc>>,
    pub target_modified: Option<DateTime<Utc>>,
    pub target_size: u32,
    /// [MS-SHLLINK] 2.3.1 DriveType 값 (2 = 이동식, 3 = 고정, 4 = 네트워크 …)
    pub drive_type: Option<u32>,
    pub drive_serial: Option<u32>,
    pub volume_label: Option<String>,
    pub local_base_path: Option<String>,
    pub net_name: Option<String>,
    pub common_path_suffix: Option<String>,
}

impl LnkInfo {
    /// 대상 경로. 로컬 경로가 있으면 그것을, 없으면 네트워크 공유 경로를 조합한다.
    pub fn target_path(&self) -> Option<String> {
        let suffix = self.common_path_suffix.as_deref().unwrap_or("");
        if let Some(base) = &self.local_base_path {
            return Some(format!("{base}{suffix}"));
        }
        if let Some(net) = &self.net_name {
            if suffix.is_empty() {
                return Some(net.clone());
            }
            return Some(format!("{}\\{}", net.trim_end_matches('\\'), suffix));
        }
        None
    }

    /// 볼륨 시리얼을 Windows가 보여주는 형태(`E02E-8A93`)로.
    pub fn drive_serial_display(&self) -> Option<String> {
        self.drive_serial.map(|s| format!("{:04X}-{:04X}", s >> 16, s & 0xFFFF))
    }

    /// 드라이브 종류 이름. 값의 의미는 [MS-SHLLINK] 2.3.1.
    pub fn drive_type_name(&self) -> Option<&'static str> {
        Some(match self.drive_type? {
            0 => "unknown",
            1 => "no_root_dir",
            2 => "removable",
            3 => "fixed",
            4 => "remote",
            5 => "cdrom",
            6 => "ramdisk",
            _ => "invalid",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LnkError {
    /// 머리 부분이 바로 가기 형식이 아니다.
    NotALink,
    /// 표시된 구조가 파일 범위를 벗어난다.
    Truncated(&'static str),
}

impl std::fmt::Display for LnkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LnkError::NotALink => write!(f, "바로 가기 형식이 아님"),
            LnkError::Truncated(what) => write!(f, "파일이 잘렸거나 손상됨 ({what})"),
        }
    }
}

/// .lnk 파일 하나를 해석한다.
///
/// `ansi`는 대상 PC의 ANSI 코드 페이지다(한국어 Windows는 949). 유니코드 경로가 함께
/// 들어 있으면 그것을 우선 쓰므로, 이 값은 유니코드 경로가 없는 파일에서만 영향을 준다.
pub fn parse(data: &[u8], ansi: &'static Encoding) -> Result<LnkInfo, LnkError> {
    // --- 2.1 ShellLinkHeader ---
    if bytes::u32_le(data, 0) != Some(0x4C) {
        return Err(LnkError::NotALink);
    }
    if bytes::slice(data, 4, 16) != Some(&LINK_CLSID[..]) {
        return Err(LnkError::NotALink);
    }
    let link_flags = bytes::u32_le(data, 20).ok_or(LnkError::Truncated("header"))?;
    let created = bytes::u64_le(data, 28).ok_or(LnkError::Truncated("header"))?;
    let accessed = bytes::u64_le(data, 36).ok_or(LnkError::Truncated("header"))?;
    let modified = bytes::u64_le(data, 44).ok_or(LnkError::Truncated("header"))?;
    let target_size = bytes::u32_le(data, 52).ok_or(LnkError::Truncated("header"))?;
    if data.len() < HEADER_SIZE {
        return Err(LnkError::Truncated("header"));
    }

    let mut info = LnkInfo {
        link_flags,
        target_created_raw: created,
        target_accessed_raw: accessed,
        target_modified_raw: modified,
        target_created: filetime_to_utc(created),
        target_accessed: filetime_to_utc(accessed),
        target_modified: filetime_to_utc(modified),
        target_size,
        drive_type: None,
        drive_serial: None,
        volume_label: None,
        local_base_path: None,
        net_name: None,
        common_path_suffix: None,
    };

    let mut pos = HEADER_SIZE;

    // --- 2.2 LinkTargetIDList: 크기만 읽고 건너뛴다 ---
    if link_flags & FLAG_HAS_LINK_TARGET_ID_LIST != 0 {
        let size = bytes::u16_le(data, pos).ok_or(LnkError::Truncated("id list size"))?;
        pos = pos
            .checked_add(2)
            .and_then(|p| p.checked_add(usize::from(size)))
            .ok_or(LnkError::Truncated("id list"))?;
        if pos > data.len() {
            return Err(LnkError::Truncated("id list"));
        }
    }

    // --- 2.3 LinkInfo ---
    if link_flags & FLAG_HAS_LINK_INFO != 0 {
        parse_link_info(data, pos, ansi, &mut info)?;
    }

    Ok(info)
}

fn parse_link_info(
    data: &[u8],
    base: usize,
    ansi: &'static Encoding,
    info: &mut LnkInfo,
) -> Result<(), LnkError> {
    let size = to_usize(bytes::u32_le(data, base).ok_or(LnkError::Truncated("link info size"))?)?;
    // 이후의 모든 읽기는 LinkInfo 구조 안으로 제한한다.
    let li = bytes::slice(data, base, size).ok_or(LnkError::Truncated("link info"))?;

    let header_size = bytes::u32_le(li, 4).ok_or(LnkError::Truncated("link info header"))?;
    let flags = bytes::u32_le(li, 8).ok_or(LnkError::Truncated("link info header"))?;
    let volume_id_off = bytes::u32_le(li, 12).ok_or(LnkError::Truncated("link info header"))?;
    let local_base_off = bytes::u32_le(li, 16).ok_or(LnkError::Truncated("link info header"))?;
    let net_link_off = bytes::u32_le(li, 20).ok_or(LnkError::Truncated("link info header"))?;
    let suffix_off = bytes::u32_le(li, 24).ok_or(LnkError::Truncated("link info header"))?;
    // LinkInfoHeaderSize가 0x24 이상이면 유니코드 오프셋 두 개가 더 있다.
    let (local_base_off_u, suffix_off_u) = if header_size >= 0x24 {
        (bytes::u32_le(li, 28), bytes::u32_le(li, 32))
    } else {
        (None, None)
    };

    if flags & LINKINFO_VOLUME_ID_AND_LOCAL_BASE_PATH != 0 {
        if volume_id_off != 0 {
            parse_volume_id(li, to_usize(volume_id_off)?, ansi, info);
        }
        info.local_base_path = match local_base_off_u {
            Some(off) if off != 0 => read_utf16(li, off),
            _ => read_ansi(li, local_base_off, ansi),
        };
    }

    if flags & LINKINFO_COMMON_NETWORK_RELATIVE_LINK != 0 && net_link_off != 0 {
        info.net_name = parse_net_name(li, to_usize(net_link_off)?, ansi);
    }

    info.common_path_suffix = match suffix_off_u {
        Some(off) if off != 0 => read_utf16(li, off),
        _ => read_ansi(li, suffix_off, ansi),
    }
    .filter(|s| !s.is_empty());

    Ok(())
}

/// 2.3.1 VolumeID. 구조가 깨져 있으면 읽을 수 있는 데까지만 채운다.
fn parse_volume_id(li: &[u8], off: usize, ansi: &'static Encoding, info: &mut LnkInfo) {
    let Some(size) = bytes::u32_le(li, off).and_then(|s| usize::try_from(s).ok()) else {
        return;
    };
    let Some(vol) = bytes::slice(li, off, size) else {
        return;
    };
    info.drive_type = bytes::u32_le(vol, 4);
    info.drive_serial = bytes::u32_le(vol, 8);
    let Some(label_off) = bytes::u32_le(vol, 12) else {
        return;
    };
    // VolumeLabelOffset이 0x14이면 그 뒤에 유니코드 오프셋이 있고 그것을 써야 한다.
    info.volume_label = if label_off == 0x14 {
        bytes::u32_le(vol, 16).and_then(|u| read_utf16(vol, u))
    } else {
        read_ansi(vol, label_off, ansi)
    }
    .filter(|s| !s.is_empty());
}

/// 2.3.2 CommonNetworkRelativeLink에서 공유 이름(`\\server\share`)만 읽는다.
fn parse_net_name(li: &[u8], off: usize, ansi: &'static Encoding) -> Option<String> {
    let size = usize::try_from(bytes::u32_le(li, off)?).ok()?;
    let net = bytes::slice(li, off, size)?;
    let net_name_off = bytes::u32_le(net, 8)?;
    // NetNameOffset이 0x14보다 크면 유니코드 오프셋이 있다.
    if net_name_off > 0x14 {
        if let Some(s) = bytes::u32_le(net, 20).filter(|&u| u != 0).and_then(|u| read_utf16(net, u))
        {
            return Some(s);
        }
    }
    read_ansi(net, net_name_off, ansi)
}

fn to_usize(v: u32) -> Result<usize, LnkError> {
    usize::try_from(v).map_err(|_| LnkError::Truncated("offset"))
}

fn read_ansi(buf: &[u8], off: u32, ansi: &'static Encoding) -> Option<String> {
    if off == 0 {
        return None;
    }
    let raw = bytes::bytes_until_nul(buf, usize::try_from(off).ok()?, MAX_STRING_BYTES)?;
    let (text, _, _) = ansi.decode(raw);
    Some(text.into_owned())
}

fn read_utf16(buf: &[u8], off: u32) -> Option<String> {
    bytes::utf16le_until_nul(buf, usize::try_from(off).ok()?, MAX_STRING_BYTES / 2)
}
