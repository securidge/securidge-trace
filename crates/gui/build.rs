//! Windows 실행 파일에 매니페스트를 넣는다.
//! - 관리자 계정이면 실행할 때 권한 상승을 묻는다 (highestAvailable). 일반 계정이면 그대로 실행된다.
//! - 고해상도 화면에서 글자가 흐려지지 않도록 DPI 인식을 켠다.
use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let m = new_manifest("Securidge.Trace")
            .requested_execution_level(ExecutionLevel::HighestAvailable)
            .dpi_awareness(DpiAwareness::PerMonitorV2);
        if let Err(e) = embed_manifest(m) {
            println!("cargo:warning=매니페스트를 넣지 못함: {e}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
