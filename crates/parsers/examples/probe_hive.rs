#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use st_parsers::hive::Hive;
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let mut h = Hive::open(std::path::Path::new(&p)).unwrap();
    println!("log: {:?}", h.log_note);
    let cs = h.current_control_set();
    println!("cs={cs}");
    for path in [format!("{cs}\\Control\\ComputerName\\ComputerName"), format!("{cs}\\Control\\TimeZoneInformation"), format!("{cs}\\Control\\Nls\\CodePage"), format!("{cs}\\Control\\Windows")] {
        if let Some(k) = h.key(&path) { println!("[{path}] values={:?}", k.value_names()); }
        else { println!("[{path}] 없음"); }
    }
    let k = h.key(&format!("{cs}\\Control\\TimeZoneInformation")).unwrap();
    println!(" tz={:?} bias={:?}", k.string("TimeZoneKeyName"), k.u32("Bias"));
    println!(" acp={:?}", h.key(&format!("{cs}\\Control\\Nls\\CodePage")).and_then(|k| k.string("ACP")));
    if let Some(mut usb) = h.key(&format!("{cs}\\Enum\\USBSTOR")) {
        for mut d in h.subkeys(&mut usb) {
            for mut inst in h.subkeys(&mut d) {
                println!("USBSTOR {} / {} fn={:?} lw={}", d.name(), inst.name(), inst.string("FriendlyName"), inst.last_written());
                if let Some(mut props) = h.subkey(&mut inst, "Properties\\{83da6326-97a6-4088-9453-a1923f573b29}") {
                    for p in h.subkeys(&mut props) { println!("   prop {} vals={:?} ft={:?}", p.name(), p.value_names(), p.first_filetime()); }
                } else { println!("   Properties 없음"); }
            }
        }
    }
    if let Some(mut usb) = h.key(&format!("{cs}\\Enum\\USB")) {
        for mut d in h.subkeys(&mut usb).into_iter().take(40) {
            for inst in h.subkeys(&mut d) {
                println!("USB {} / {} class={:?} svc={:?} desc={:?}", d.name(), inst.name(), inst.string("ClassGUID"), inst.string("Service"), inst.string("DeviceDesc"));
            }
        }
    }
    if let Some(m) = h.key("MountedDevices") {
        for n in m.value_names() { let b = m.binary(&n).unwrap_or_default(); let s: String = if b.len()>24 { String::from_utf16_lossy(&b.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()) } else { format!("{:02x?}", b) }; println!("MD {n} = {}", s.chars().take(110).collect::<String>()); }
    }
    if let Some(mut bt) = h.key(&format!("{cs}\\Services\\BTHPORT\\Parameters\\Devices")) {
        for d in h.subkeys(&mut bt) { println!("BT {} vals={:?}", d.name(), d.value_names()); }
    } else { println!("BT 없음"); }
}
