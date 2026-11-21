"""Cargo.lock에서 notatin이 nom 7.1.3을 쓰도록 고정한다.

notatin 1.0.1은 `nom >= 6`으로 선언돼 있지만 nom 8에서는 컴파일되지 않는다. 다른 crate가 nom 8을
쓰면 Cargo가 의존성을 다시 풀 때 notatin도 nom 8로 묶어 버리므로, 의존성을 바꾼 뒤에는 이 스크립트를
한 번 돌린다. (nom 7.1.3도 notatin의 `>= 6` 조건을 만족하므로 Cargo가 그대로 받아들인다.)
"""
import pathlib
import re

p = pathlib.Path(__file__).resolve().parent.parent / "Cargo.lock"
s = p.read_text(encoding="utf-8")

NOM7 = '''[[package]]
name = "nom"
version = "7.1.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "d273983c5a657a70a3e8f2a01329822f3b8c8172b73826411a55751e404a0a4a"
dependencies = [
 "memchr",
 "minimal-lexical",
]

'''
MINLEX = '''[[package]]
name = "minimal-lexical"
version = "0.2.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "68354c5c6bd36d73ff3feceb05efa59b6acb7626617f4962be322a825e61f79a"

'''

if 'name = "nom"\nversion = "7.1.3"' not in s:
    s = s.replace('[[package]]\nname = "nom"\nversion = "8.0.0"', NOM7 + '[[package]]\nname = "nom"\nversion = "8.0.0"', 1)
if 'name = "minimal-lexical"' not in s:
    s = s.replace('[[package]]\nname = "miniz_oxide"', MINLEX + '[[package]]\nname = "miniz_oxide"', 1)

# 패키지 블록별로 nom 참조를 고친다: notatin → "nom 7.1.3", 나머지 → "nom 8.0.0"
blocks = s.split("\n[[package]]\n")
out = []
for b in blocks:
    name = re.search(r'^name = "([^"]+)"', b, re.M) or re.search(r'name = "([^"]+)"', b)
    n = name.group(1) if name else ""
    target = '"nom 7.1.3"' if n == "notatin" else '"nom 8.0.0"'
    b = re.sub(r'^ "nom( [0-9.]+)?",$', " " + target + ",", b, flags=re.M)
    out.append(b)
s = "\n[[package]]\n".join(out)
p.write_text(s, encoding="utf-8")
print("Cargo.lock: notatin → nom 7.1.3")
