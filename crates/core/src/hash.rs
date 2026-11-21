//! SHA-256 해시. 입력 파일이 분석 전후로 바뀌지 않았음을 보이는 데 쓴다.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// 바이트열의 SHA-256 (소문자 16진수).
pub fn sha256_bytes(data: &[u8]) -> String {
    to_hex(&Sha256::digest(data))
}

/// 파일의 SHA-256 (소문자 16진수). 큰 파일도 메모리에 다 올리지 않는다.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(chunk) = buf.get(..n) {
            hasher.update(chunk);
        }
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // NIST FIPS 180-4 예제: "abc"
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
