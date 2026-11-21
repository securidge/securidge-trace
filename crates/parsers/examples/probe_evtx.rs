#![allow(clippy::unwrap_used)]
fn main() {
    for p in std::env::args().skip(1) {
        let r = st_parsers::eventlog::read(std::path::Path::new(&p)).unwrap();
        println!("{p}: total={} bad={} picked={} range={:?}~{:?}", r.total_records, r.bad_records, r.events.len(), r.earliest, r.latest);
        for e in r.events.iter().take(4) { println!("  {} {:?} {} {:?}", e.event_id, e.time, e.channel, e.data); }
    }
}
