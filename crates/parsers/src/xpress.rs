//! Xpress Huffman 압축 풀기 (Windows 10 이후 Prefetch 파일이 쓰는 압축).
//!
//! 근거: Microsoft 공개 규격 [MS-XCA] "Xpress Compression Algorithm"
//!   2.1 LZ77+Huffman 압축 형식, 2.2.4 압축 풀기 의사 코드.
//! 구현 순서는 Eric Zimmerman의 Prefetch 라이브러리(MIT, `Xpress2.cs`)와 같고, 그 라이브러리의
//! Windows 10 표본 파일로 결과를 확인한다 (tests/prefetch_samples.rs).
//!
//! 형식 요약
//! - 출력 64KiB마다 256바이트 표가 앞에 온다. 표는 기호 512개의 부호 길이(4비트씩)다.
//! - 기호 0~255는 문자 그대로, 256~511은 (길이, 거리) 쌍이다.
//! - 비트는 16비트 단위(리틀 엔디언)로 읽고, 길이 확장 바이트는 비트 흐름 사이에 끼어 있다.

const TABLE_BYTES: usize = 256;
const SYMBOLS: usize = 512;
const MAX_CODE_LEN: u32 = 15;
const BLOCK: usize = 65_536;
/// 풀어낸 결과 크기의 상한 (조작된 파일이 메모리를 과하게 쓰지 못하게).
pub const MAX_OUTPUT: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XpressError(pub &'static str);

impl std::fmt::Display for XpressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "압축을 풀지 못함 ({})", self.0)
    }
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    next: u32,
    extra: i32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize) -> Result<Self, XpressError> {
        let a = u16_at(data, pos).ok_or(XpressError("비트 흐름 시작"))?;
        let b = u16_at(data, pos + 2).ok_or(XpressError("비트 흐름 시작"))?;
        Ok(Bits { data, pos: pos + 4, next: (u32::from(a) << 16) | u32::from(b), extra: 16 })
    }

    fn peek(&self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next >> (32 - n)
        }
    }

    fn consume(&mut self, n: u32) -> Result<(), XpressError> {
        if n == 0 {
            return Ok(());
        }
        self.next <<= n;
        self.extra -= i32::try_from(n).map_err(|_| XpressError("비트 수"))?;
        if self.extra < 0 {
            let w = u16_at(self.data, self.pos).ok_or(XpressError("비트 흐름이 끝을 넘음"))?;
            self.next |= u32::from(w) << (-self.extra);
            self.extra += 16;
            self.pos += 2;
        }
        Ok(())
    }

    fn byte(&mut self) -> Result<u8, XpressError> {
        let b = *self.data.get(self.pos).ok_or(XpressError("길이 바이트가 끝을 넘음"))?;
        self.pos += 1;
        Ok(b)
    }

    fn word(&mut self) -> Result<u16, XpressError> {
        let w = u16_at(self.data, self.pos).ok_or(XpressError("길이 값이 끝을 넘음"))?;
        self.pos += 2;
        Ok(w)
    }
}

fn u16_at(d: &[u8], p: usize) -> Option<u16> {
    let b = d.get(p..p.checked_add(2)?)?;
    Some(u16::from_le_bytes([*b.first()?, *b.get(1)?]))
}

/// 표준(canonical) 허프만 부호 길이로 15비트 조회표를 만든다.
fn build_table(lengths: &[u8; SYMBOLS], table: &mut [u16]) -> Result<(), XpressError> {
    let mut next = 0usize;
    for len in 1..=MAX_CODE_LEN {
        let span = 1usize << (MAX_CODE_LEN - len);
        for (sym, &l) in lengths.iter().enumerate() {
            if u32::from(l) != len {
                continue;
            }
            let end = next.checked_add(span).ok_or(XpressError("부호 표"))?;
            let slot = table.get_mut(next..end).ok_or(XpressError("부호 길이가 너무 많음"))?;
            slot.fill(u16::try_from(sym).map_err(|_| XpressError("기호"))?);
            next = end;
        }
    }
    if next == 0 {
        return Err(XpressError("부호 표가 비어 있음"));
    }
    let last = *table.get(next - 1).ok_or(XpressError("부호 표"))?;
    if let Some(rest) = table.get_mut(next..) {
        rest.fill(last);
    }
    Ok(())
}

/// `data`를 풀어 `size`바이트를 돌려준다.
pub fn decompress(data: &[u8], size: usize) -> Result<Vec<u8>, XpressError> {
    if size > MAX_OUTPUT {
        return Err(XpressError("풀어낸 크기가 너무 큼"));
    }
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut lengths = [0u8; SYMBOLS];
    let mut table = vec![0u16; 1 << MAX_CODE_LEN];
    let mut in_pos = 0usize;

    while out.len() < size {
        let tbl = data.get(in_pos..in_pos + TABLE_BYTES).ok_or(XpressError("부호 표가 잘림"))?;
        for (i, b) in tbl.iter().enumerate() {
            if let Some(x) = lengths.get_mut(i * 2) {
                *x = b & 0x0F;
            }
            if let Some(x) = lengths.get_mut(i * 2 + 1) {
                *x = b >> 4;
            }
        }
        build_table(&lengths, &mut table)?;
        let mut bits = Bits::new(data, in_pos + TABLE_BYTES)?;
        let block_end = (out.len() + BLOCK).min(size);

        while out.len() < block_end {
            let idx = usize::try_from(bits.peek(MAX_CODE_LEN)).map_err(|_| XpressError("비트"))?;
            let sym = *table.get(idx).ok_or(XpressError("부호"))?;
            let sym_len = *lengths.get(usize::from(sym)).ok_or(XpressError("부호 길이"))?;
            bits.consume(u32::from(sym_len))?;
            if sym < 256 {
                out.push(u8::try_from(sym).map_err(|_| XpressError("문자"))?);
                continue;
            }
            let s = sym - 256;
            let mut length = usize::from(s & 0x0F);
            let offset_bits = u32::from(s >> 4);
            if length == 15 {
                length = usize::from(bits.byte()?);
                if length == 255 {
                    let w = usize::from(bits.word()?);
                    if w < 15 {
                        return Err(XpressError("길이 확장 값이 잘못됨"));
                    }
                    length = w - 15;
                }
                length += 15;
            }
            length += 3;
            let offset = if offset_bits == 0 {
                1
            } else {
                usize::try_from(bits.peek(offset_bits)).map_err(|_| XpressError("거리"))? + (1usize << offset_bits)
            };
            bits.consume(offset_bits)?;
            if offset > out.len() {
                return Err(XpressError("거리가 출력 시작보다 앞을 가리킴"));
            }
            let copy_end = (out.len() + length).min(size);
            while out.len() < copy_end {
                let b = *out.get(out.len() - offset).ok_or(XpressError("복사"))?;
                out.push(b);
            }
        }
        in_pos = bits.pos;
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage_without_panic() {
        assert!(decompress(&[], 10).is_err());
        assert!(decompress(&[0u8; 300], 10).is_err(), "부호 길이가 전부 0이면 실패");
        assert!(decompress(&[0x11u8; 300], 100).is_err() || decompress(&[0x11u8; 300], 100).is_ok());
        assert!(decompress(&[0u8; 10], MAX_OUTPUT + 1).is_err());
    }
}
