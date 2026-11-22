//! Windows 실행 파일에 아이콘과 매니페스트를 넣는다.
//! - 아이콘: assets/icon.ico (16~256px)
//! - 관리자 계정이면 실행할 때 권한 상승을 묻는다 (highestAvailable). 일반 계정이면 그대로 실행된다.
//! - 고해상도 화면에서 글자가 흐려지지 않도록 DPI 인식을 켠다.
use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
use embed_manifest::new_manifest;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = new_manifest("Securidge.Trace")
        .requested_execution_level(ExecutionLevel::HighestAvailable)
        .dpi_awareness(DpiAwareness::PerMonitorV2)
        .to_string();
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icon.ico")
        .set("ProductName", "Securidge Trace")
        .set("FileDescription", "Securidge Trace")
        .set("CompanyName", "Kwangmo Kim")
        .set("LegalCopyright", "© Kwangmo Kim (김광모). MIT OR Apache-2.0")
        .set_manifest(&manifest);
    if let Err(e) = res.compile() {
        println!("cargo:warning=아이콘·매니페스트를 넣지 못함: {e}");
    }
}
