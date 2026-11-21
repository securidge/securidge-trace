//! st-parsers: 아티팩트 파일을 읽어 구조화된 레코드로 바꾼다.
//!
//! 규칙 (개발 요청서 2장 3번)
//! - 입력은 손상됐거나 조작됐을 수 있다. 어떤 입력에도 패닉하지 않는다.
//! - `unwrap()`/`expect()`/직접 인덱싱을 쓰지 않는다 (workspace lint로 강제).
//! - 길이·개수에는 상한을 둔다.
//! - 각 파서 파일 맨 위에 근거 문서를 적는다.

pub mod browser;
pub mod lnk;
pub mod recyclebin;
