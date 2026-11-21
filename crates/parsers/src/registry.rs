//! SYSTEM·SOFTWARE 하이브에서 시스템 정보, 계정, 장치, 네트워크를 읽는다.
//!
//! 근거 (경로·값 이름은 아래 자료와 Windows Server 2022/2025 실기 확인으로 정했다)
//! - 시스템: `<CS>\Control\ComputerName\ComputerName`, `<CS>\Control\TimeZoneInformation`,
//!   `<CS>\Control\Nls\CodePage\ACP`, `<CS>\Control\Windows\ShutdownTime`(FILETIME),
//!   SOFTWARE `Microsoft\Windows NT\CurrentVersion` (ProductName, CurrentBuild, DisplayVersion, InstallDate=유닉스 초)
//! - 계정: SOFTWARE `Microsoft\Windows NT\CurrentVersion\ProfileList\<SID>\ProfileImagePath`
//! - USB 저장장치: `<CS>\Enum\USBSTOR\<종류>\<인스턴스>` 와 그 아래
//!   `Properties\{83da6326-97a6-4088-9453-a1923f573b29}\0064`(최초 설치), `0066`(마지막 연결), `0067`(마지막 제거)
//!   (Microsoft DEVPKEY_Device_InstallDate / LastArrivalDate / LastRemovalDate, Windows 8 이상)
//! - 휴대 장치(스마트폰 등 MTP/PTP): `<CS>\Enum\USB\<VID_PID>\<인스턴스>` 중 ClassGUID가
//!   WPD {eec5ad98-8080-425f-922a-dabf3de3f69a} 또는 Image {6bdd1fc6-810f-11d0-bec7-08002be2092f}
//! - 드라이브 문자: SYSTEM `MountedDevices` (`\DosDevices\E:` → 장치 경로 UTF-16)
//! - 네트워크: SOFTWARE `Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles\<GUID>`
//!   (ProfileName, NameType: 0x06 유선, 0x47 무선, 0x17·0xF3 모바일, DateCreated·DateLastConnected = SYSTEMTIME)
//!   `…\NetworkList\Signatures\{Managed,Unmanaged}\<id>` (ProfileGuid, DefaultGatewayMac)
//!   SYSTEMTIME 값은 Microsoft 문서(INetwork::GetTimeCreatedAndConnected)상 로컬 시간이다.
//! - 블루투스: `<CS>\Services\BTHPORT\Parameters\Devices\<주소>` (Name, LastSeen, LastConnected)
//!
//! 검증 상태는 README의 "검증 현황"을 따른다.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use st_core::time::filetime_to_utc;

use crate::hive::{Hive, Key};

const DEVPROP_GUID: &str = "{83da6326-97a6-4088-9453-a1923f573b29}";
const CLASS_WPD: &str = "{eec5ad98-8080-425f-922a-dabf3de3f69a}";
const CLASS_IMAGE: &str = "{6bdd1fc6-810f-11d0-bec7-08002be2092f}";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemInfo {
    pub computer_name: Option<String>,
    pub product_name: Option<String>,
    pub build: Option<String>,
    pub display_version: Option<String>,
    pub install_date: Option<DateTime<Utc>>,
    pub last_shutdown: Option<DateTime<Utc>>,
    pub time_zone_key: Option<String>,
    /// 표준시 기준 UTC와의 차이(분). Windows의 Bias는 "UTC = 로컬 + Bias"이므로 부호를 뒤집어 저장한다.
    pub utc_offset_minutes: Option<i32>,
    /// 일광 절약 시간을 쓰는 시간대인가 (쓰면 로컬 시각 변환에 1시간 오차가 생길 수 있다)
    pub uses_dst: bool,
    pub ansi_codepage: Option<u32>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub sid: String,
    pub profile_path: String,
    /// 프로필 폴더 이름 (보통 계정 이름과 같다)
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    /// USB 저장장치 (메모리, 외장 디스크)
    UsbStorage,
    /// 스마트폰·카메라 등 휴대 장치 (MTP/PTP)
    Portable,
    Bluetooth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub kind: DeviceKind,
    /// 레지스트리 키 경로 (출처)
    pub key_path: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub friendly_name: Option<String>,
    /// 장치가 알려준 일련번호. 두 번째 글자가 '&'이면 Windows가 만든 값이라 장치 고유 번호가 아니다.
    pub serial: Option<String>,
    pub serial_is_unique: bool,
    pub first_install: Option<DateTime<Utc>>,
    pub last_arrival: Option<DateTime<Utc>>,
    pub last_removal: Option<DateTime<Utc>>,
    /// 장치 정보 키의 마지막 기록 시각 (보조 근거)
    pub key_last_written: Option<DateTime<Utc>>,
    /// MountedDevices에 남은 드라이브 문자 (현재 또는 마지막 할당)
    pub drive_letters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub profile_guid: String,
    pub name: String,
    pub description: Option<String>,
    /// "wired", "wireless", "mobile", "unknown"
    pub medium: String,
    pub name_type: Option<u32>,
    pub created_local_raw: Option<String>,
    pub last_connected_local_raw: Option<String>,
    pub created_utc: Option<DateTime<Utc>>,
    pub last_connected_utc: Option<DateTime<Utc>>,
    pub gateway_mac: Option<String>,
}

pub fn system_info(system: &mut Hive, software: Option<&mut Hive>) -> SystemInfo {
    let mut info = SystemInfo::default();
    let cs = system.current_control_set();
    info.computer_name = system
        .key(&format!("{cs}\\Control\\ComputerName\\ComputerName"))
        .and_then(|k| k.string("ComputerName"));
    if let Some(tz) = system.key(&format!("{cs}\\Control\\TimeZoneInformation")) {
        info.time_zone_key = tz.string("TimeZoneKeyName");
        info.utc_offset_minutes = tz.u32("Bias").map(|b| -(b as i32));
        // DaylightStart(SYSTEMTIME)의 월 값이 0이 아니면 일광 절약 시간을 쓴다.
        info.uses_dst = tz
            .binary("DaylightStart")
            .and_then(|b| st_core::bytes::u16_le(&b, 2))
            .map(|month| month != 0)
            .unwrap_or(false)
            && tz.u32("DynamicDaylightTimeDisabled") != Some(1);
    } else {
        info.notes.push("시간대 정보를 찾지 못함".to_string());
    }
    info.ansi_codepage = system
        .key(&format!("{cs}\\Control\\Nls\\CodePage"))
        .and_then(|k| k.string("ACP"))
        .and_then(|s| s.trim().parse().ok());
    info.last_shutdown = system
        .key(&format!("{cs}\\Control\\Windows"))
        .and_then(|k| k.u64("ShutdownTime"))
        .and_then(filetime_to_utc);

    if let Some(sw) = software {
        if let Some(k) = sw.key("Microsoft\\Windows NT\\CurrentVersion") {
            info.product_name = k.string("ProductName");
            info.build = k.string("CurrentBuild").or_else(|| k.string("CurrentBuildNumber"));
            info.display_version = k.string("DisplayVersion");
            info.install_date = k
                .u32("InstallDate")
                .filter(|&s| s > 0)
                .and_then(|s| DateTime::from_timestamp(i64::from(s), 0));
        }
    }
    info
}

pub fn accounts(software: &mut Hive) -> Vec<Account> {
    let mut out = Vec::new();
    let Some(mut list) = software.key("Microsoft\\Windows NT\\CurrentVersion\\ProfileList") else {
        return out;
    };
    for k in software.subkeys(&mut list) {
        let Some(path) = k.string("ProfileImagePath") else { continue };
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
        out.push(Account { sid: k.name().to_string(), profile_path: path, name });
    }
    out
}

/// `Disk&Ven_SanDisk&Prod_Ultra&Rev_1.00` → (SanDisk, Ultra)
fn vendor_product(device_class: &str) -> (Option<String>, Option<String>) {
    let mut vendor = None;
    let mut product = None;
    for part in device_class.split('&') {
        if let Some(v) = part.strip_prefix("Ven_") {
            vendor = Some(v.replace('_', " ").trim().to_string()).filter(|s| !s.is_empty());
        } else if let Some(p) = part.strip_prefix("Prod_") {
            product = Some(p.replace('_', " ").trim().to_string()).filter(|s| !s.is_empty());
        }
    }
    (vendor, product)
}

/// USBSTOR 인스턴스 이름 `4C530001230606104182&0` → 일련번호 `4C530001230606104182`
fn instance_serial(instance: &str) -> (String, bool) {
    let unique = instance.chars().nth(1) != Some('&');
    let serial = if unique {
        instance.rsplit_once('&').map(|(s, _)| s).unwrap_or(instance).to_string()
    } else {
        instance.to_string()
    };
    (serial, unique)
}

fn device_times(hive: &mut Hive, inst: &mut Key, dev: &mut Device) {
    let Some(mut props) = hive.subkey(inst, &format!("Properties\\{DEVPROP_GUID}")) else {
        return;
    };
    for p in hive.subkeys(&mut props) {
        let t = p.first_filetime().and_then(filetime_to_utc);
        match p.name() {
            "0064" => dev.first_install = t,
            "0066" => dev.last_arrival = t,
            "0067" => dev.last_removal = t,
            _ => {}
        }
    }
}

/// MountedDevices에서 드라이브 문자 → 장치 경로 문자열.
pub fn mounted_devices(system: &mut Hive) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(k) = system.key("MountedDevices") else { return out };
    for name in k.value_names() {
        let Some(letter) = name.strip_prefix("\\DosDevices\\") else { continue };
        let Some(data) = k.binary(&name) else { continue };
        // 장치 경로는 UTF-16LE로 저장된다. 그렇지 않은 값(디스크 서명 12바이트 등)은 건너뛴다.
        if data.len() < 16 || data.first() != Some(&b'_') {
            continue;
        }
        let units: Vec<u16> =
            data.chunks_exact(2).filter_map(|c| c.try_into().ok().map(u16::from_le_bytes)).collect();
        out.push((letter.to_string(), String::from_utf16_lossy(&units)));
    }
    out.sort();
    out
}

pub fn usb_devices(system: &mut Hive) -> Vec<Device> {
    let cs = system.current_control_set();
    let mounts = mounted_devices(system);
    let mut out = Vec::new();

    if let Some(mut usbstor) = system.key(&format!("{cs}\\Enum\\USBSTOR")) {
        for mut class in system.subkeys(&mut usbstor) {
            let class_name = class.name().to_string();
            let (vendor, product) = vendor_product(&class_name);
            for mut inst in system.subkeys(&mut class) {
                let (serial, unique) = instance_serial(inst.name());
                let mut dev = Device {
                    kind: DeviceKind::UsbStorage,
                    key_path: format!("{cs}\\Enum\\USBSTOR\\{class_name}\\{}", inst.name()),
                    vendor: vendor.clone(),
                    product: product.clone(),
                    friendly_name: inst.string("FriendlyName"),
                    serial: Some(serial.clone()),
                    serial_is_unique: unique,
                    first_install: None,
                    last_arrival: None,
                    last_removal: None,
                    key_last_written: Some(inst.last_written()),
                    drive_letters: Vec::new(),
                };
                device_times(system, &mut inst, &mut dev);
                let needle = format!("#{}#", inst.name()).to_ascii_lowercase();
                dev.drive_letters = mounts
                    .iter()
                    .filter(|(_, path)| path.to_ascii_lowercase().contains(&needle))
                    .map(|(l, _)| l.clone())
                    .collect();
                out.push(dev);
            }
        }
    }

    if let Some(mut usb) = system.key(&format!("{cs}\\Enum\\USB")) {
        for mut vidpid in system.subkeys(&mut usb) {
            let vp = vidpid.name().to_string();
            for mut inst in system.subkeys(&mut vidpid) {
                let class = inst.string("ClassGUID").unwrap_or_default().to_ascii_lowercase();
                if class != CLASS_WPD && class != CLASS_IMAGE {
                    continue;
                }
                let (serial, unique) = instance_serial(inst.name());
                let mut dev = Device {
                    kind: DeviceKind::Portable,
                    key_path: format!("{cs}\\Enum\\USB\\{vp}\\{}", inst.name()),
                    vendor: inst.string("Mfg").map(clean_inf_string),
                    product: inst.string("DeviceDesc").map(clean_inf_string),
                    friendly_name: inst.string("FriendlyName"),
                    serial: Some(serial),
                    serial_is_unique: unique,
                    first_install: None,
                    last_arrival: None,
                    last_removal: None,
                    key_last_written: Some(inst.last_written()),
                    drive_letters: Vec::new(),
                };
                device_times(system, &mut inst, &mut dev);
                out.push(dev);
            }
        }
    }
    out
}

/// `@wpdmtp.inf,%mtp.devicedesc%;MTP USB Device` → `MTP USB Device`
fn clean_inf_string(s: String) -> String {
    match s.rsplit_once(';') {
        Some((_, tail)) => tail.to_string(),
        None => s,
    }
}

pub fn bluetooth_devices(system: &mut Hive) -> Vec<Device> {
    let cs = system.current_control_set();
    let mut out = Vec::new();
    let Some(mut devs) = system.key(&format!("{cs}\\Services\\BTHPORT\\Parameters\\Devices")) else {
        return out;
    };
    for k in system.subkeys(&mut devs) {
        let name = k.binary("Name").map(|b| {
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf8_lossy(b.get(..end).unwrap_or(&[])).into_owned()
        });
        let mac = k.name().to_ascii_uppercase();
        let mac = if mac.len() == 12 {
            mac.as_bytes().chunks(2).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(":")
        } else {
            mac
        };
        out.push(Device {
            kind: DeviceKind::Bluetooth,
            key_path: format!("{cs}\\Services\\BTHPORT\\Parameters\\Devices\\{}", k.name()),
            vendor: None,
            product: None,
            friendly_name: name,
            serial: Some(mac),
            serial_is_unique: true,
            first_install: None,
            last_arrival: k.u64("LastConnected").and_then(filetime_to_utc),
            last_removal: None,
            key_last_written: Some(k.last_written()),
            drive_letters: Vec::new(),
        });
    }
    out
}

/// SYSTEMTIME(16바이트) → (표시용 문자열, NaiveDateTime)
fn systemtime(b: &[u8]) -> Option<(String, chrono::NaiveDateTime)> {
    let g = |i: usize| st_core::bytes::u16_le(b, i * 2).map(u32::from);
    let (y, mo, d, h, mi, s) = (g(0)?, g(1)?, g(3)?, g(4)?, g(5)?, g(6)?);
    let dt = NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, mo, d)?.and_hms_opt(h, mi, s)?;
    Some((dt.format("%Y-%m-%d %H:%M:%S").to_string(), dt))
}

/// 로컬 시각 → UTC (표준시 기준 오프셋 사용)
fn local_to_utc(local: chrono::NaiveDateTime, offset_min: Option<i32>) -> Option<DateTime<Utc>> {
    let off = chrono::FixedOffset::east_opt(offset_min? * 60)?;
    off.from_local_datetime(&local).single().map(|d| d.with_timezone(&Utc))
}

pub fn networks(software: &mut Hive, utc_offset_minutes: Option<i32>) -> Vec<Network> {
    let mut out = Vec::new();
    let base = "Microsoft\\Windows NT\\CurrentVersion\\NetworkList";

    // 게이트웨이 MAC: Signatures 아래에서 ProfileGuid로 연결
    let mut gw: Vec<(String, String)> = Vec::new();
    for sub in ["Managed", "Unmanaged"] {
        let Some(mut sig) = software.key(&format!("{base}\\Signatures\\{sub}")) else { continue };
        for k in software.subkeys(&mut sig) {
            if let (Some(g), Some(mac)) = (k.string("ProfileGuid"), k.binary("DefaultGatewayMac")) {
                let mac = mac.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
                gw.push((g.to_ascii_lowercase(), mac));
            }
        }
    }

    let Some(mut profiles) = software.key(&format!("{base}\\Profiles")) else { return out };
    for k in software.subkeys(&mut profiles) {
        let guid = k.name().to_string();
        let name_type = k.u32("NameType");
        let medium = match name_type {
            Some(0x06) => "wired",
            Some(0x47) => "wireless",
            Some(0x17) | Some(0xF3) => "mobile",
            _ => "unknown",
        };
        let created = k.binary("DateCreated").and_then(|b| systemtime(&b));
        let last = k.binary("DateLastConnected").and_then(|b| systemtime(&b));
        out.push(Network {
            name: k.string("ProfileName").unwrap_or_default(),
            description: k.string("Description"),
            medium: medium.to_string(),
            name_type,
            created_local_raw: created.as_ref().map(|c| c.0.clone()),
            last_connected_local_raw: last.as_ref().map(|c| c.0.clone()),
            created_utc: created.and_then(|c| local_to_utc(c.1, utc_offset_minutes)),
            last_connected_utc: last.and_then(|c| local_to_utc(c.1, utc_offset_minutes)),
            gateway_mac: gw.iter().find(|(g, _)| *g == guid.to_ascii_lowercase()).map(|(_, m)| m.clone()),
            profile_guid: guid,
        });
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn parses_usbstor_names() {
        assert_eq!(
            vendor_product("Disk&Ven_SanDisk&Prod_Ultra_USB_3.0&Rev_1.00"),
            (Some("SanDisk".into()), Some("Ultra USB 3.0".into()))
        );
        assert_eq!(instance_serial("4C530001230606104182&0"), ("4C530001230606104182".into(), true));
        let (_, unique) = instance_serial("7&2a8c3f1&0");
        assert!(!unique);
    }

    #[test]
    fn systemtime_and_offset() {
        // 2026-11-03 22:12:05 (로컬, 서울 +9) → 13:12:05 UTC
        let mut b = Vec::new();
        for v in [2026u16, 11, 2, 3, 22, 12, 5, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let (raw, local) = systemtime(&b).unwrap_or_default_for_test();
        assert_eq!(raw, "2026-11-03 22:12:05");
        assert_eq!(
            local_to_utc(local, Some(540)).map(|d| d.to_rfc3339()),
            Some("2026-11-03T13:12:05+00:00".to_string())
        );
        assert!(systemtime(&[0u8; 4]).is_none());
        assert!(systemtime(&[0u8; 16]).is_none(), "0년 0월은 날짜가 아님");
    }

    trait OrDefault {
        fn unwrap_or_default_for_test(self) -> (String, chrono::NaiveDateTime);
    }
    impl OrDefault for Option<(String, chrono::NaiveDateTime)> {
        fn unwrap_or_default_for_test(self) -> (String, chrono::NaiveDateTime) {
            self.unwrap_or_else(|| (String::new(), chrono::NaiveDateTime::default()))
        }
    }

    #[test]
    fn inf_string_cleanup() {
        assert_eq!(clean_inf_string("@wpdmtp.inf,%mtp%;MTP USB Device".into()), "MTP USB Device");
        assert_eq!(clean_inf_string("Galaxy S24".into()), "Galaxy S24");
    }
}
