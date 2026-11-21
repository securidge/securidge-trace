//! 범위를 벗어나면 패닉 대신 None을 돌려주는 바이트 읽기 도구.
//!
//! 입력 파일은 손상됐거나 조작됐을 수 있다. 파서는 `data[a..b]` 같은 직접 인덱싱을
//! 쓰지 않고 반드시 이 함수들로 읽는다.

/// `offset`부터 `len`바이트를 돌려준다. 범위를 벗어나면 None.
pub fn slice(data: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    let end = offset.checked_add(len)?;
    data.get(offset..end)
}

pub fn u16_le(data: &[u8], offset: usize) -> Option<u16> {
    let b: [u8; 2] = slice(data, offset, 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(b))
}

pub fn u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let b: [u8; 4] = slice(data, offset, 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(b))
}

pub fn u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let b: [u8; 8] = slice(data, offset, 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(b))
}

/// `offset`부터 0x0000 종료 문자(또는 `max_units`개)까지 UTF-16LE 문자열을 읽는다.
/// 잘못된 서로게이트는 대체 문자(U+FFFD)로 바꾼다.
pub fn utf16le_until_nul(data: &[u8], offset: usize, max_units: usize) -> Option<String> {
    let mut units: Vec<u16> = Vec::new();
    let mut pos = offset;
    while units.len() < max_units {
        match u16_le(data, pos) {
            Some(0) | None => break,
            Some(u) => units.push(u),
        }
        pos = pos.checked_add(2)?;
    }
    // 시작 위치 자체가 범위를 벗어났으면 실패로 본다.
    if offset > data.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&units))
}

/// `offset`부터 0x00 종료 바이트(또는 `max_len`바이트)까지의 바이트열.
pub fn bytes_until_nul(data: &[u8], offset: usize, max_len: usize) -> Option<&[u8]> {
    let rest = data.get(offset..)?;
    let limit = rest.len().min(max_len);
    let window = rest.get(..limit)?;
    let end = window.iter().position(|&b| b == 0).unwrap_or(limit);
    window.get(..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_returns_none() {
        let d = [1u8, 2, 3];
        assert_eq!(u32_le(&d, 0), None);
        assert_eq!(u16_le(&d, 2), None);
        assert_eq!(slice(&d, usize::MAX, 2), None);
        assert_eq!(u16_le(&d, 1), Some(0x0302));
    }

    #[test]
    fn utf16_reads_until_nul() {
        let d = [b'A', 0, b'B', 0, 0, 0, b'C', 0];
        assert_eq!(utf16le_until_nul(&d, 0, 100).as_deref(), Some("AB"));
        assert_eq!(utf16le_until_nul(&d, 0, 1).as_deref(), Some("A"));
        assert_eq!(utf16le_until_nul(&d, 99, 10), None);
    }
}
