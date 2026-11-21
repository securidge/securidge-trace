"""배포물에 들어가는 외부 라이브러리와 라이선스 목록을 만든다 (THIRD-PARTY-LICENSES.txt).

사용: python3 tools/third_party.py > THIRD-PARTY-LICENSES.txt
Windows 실행 파일(st-cli, st-gui)에 실제로 링크되는 의존성만 센다 (개발·빌드 전용 제외).
"""
import json, subprocess, sys

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"]))
pk = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
roots = [p["id"] for p in meta["packages"] if p["name"] in ("st-cli", "st-gui")]
seen, stack = set(), list(roots)
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] in (None, "normal") for k in d["dep_kinds"]):
            stack.append(d["pkg"])
rows = sorted((pk[i]["name"], pk[i]["version"], pk[i].get("license") or "(라이선스 파일 참고)", pk[i].get("repository") or "")
              for i in seen if pk[i]["source"])
out = sys.stdout
out.write("Securidge Trace — 포함된 외부 라이브러리 / Third-party libraries\n")
out.write("각 라이브러리의 저작권과 라이선스 전문은 아래 저장소에서 확인할 수 있습니다.\n")
out.write("Copyright and full license texts are available at each repository.\n\n")
for name, ver, lic, repo in rows:
    out.write(f"{name} {ver}\n  License: {lic}\n  {repo}\n")
out.write(f"\n총 {len(rows)}개 / {len(rows)} packages\n")
