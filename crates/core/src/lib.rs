//! st-core: 모든 crate가 함께 쓰는 데이터 모델과 기본 도구.
//!
//! 설계 원칙 (개발 요청서 2장)
//! - 사실(Event)과 판단(규칙 결과)을 분리한다. 이 crate에는 "사실"만 있다.
//! - 모든 시각은 UTC로 보관하되 원본 값과 원본의 시간 기준을 함께 남긴다.
//! - 모든 사실에는 출처(어느 파일의 어느 레코드인지)와 신뢰도를 붙인다.

pub mod bytes;
pub mod hash;
pub mod model;
pub mod time;

pub use model::*;
