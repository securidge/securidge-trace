//! 명령줄 실행 파일(trace.exe)에도 같은 아이콘을 넣는다.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icon.ico")
        .set("ProductName", "Securidge Trace")
        .set("FileDescription", "Securidge Trace (command line)")
        .set("CompanyName", "Kwangmo Kim")
        .set("LegalCopyright", "© Kwangmo Kim (김광모). MIT OR Apache-2.0");
    if let Err(e) = res.compile() {
        println!("cargo:warning=아이콘을 넣지 못함: {e}");
    }
}
