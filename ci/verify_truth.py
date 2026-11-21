"""정답(truth.json)과 도구의 결과(evidence.json)를 대조한다.

반드시 맞아야 하는 것(틀리면 실패 처리)
  - 바로 가기: 대상 경로, 볼륨 시리얼, 대상 크기, 열람 시각(정답 시각 범위 안)
  - 휴지통: 원래 경로, 크기, 삭제 시각(정답 시각 범위 안), 계정 SID
  - 도구가 정상 종료(0 또는 2)

참고만 하는 것(결과를 기록하되 실패로 보지 않음)
  - 브라우저 방문 기록: 헤드리스 브라우저가 기록을 남기는지 자체가 확실하지 않다
  - 입력 불변: 실행 중인 Windows에서는 다른 프로그램이 파일을 바꿀 수 있다

사용: python verify_truth.py truth.json <결과 폴더> <요약 출력 파일>
"""

import json
import sys
from datetime import datetime, timedelta
from pathlib import Path

SLACK = timedelta(seconds=3)


def parse(ts):
    if not ts:
        return None
    ts = ts.replace("Z", "+00:00")
    # PowerShell의 'o' 형식은 소수점 7자리라 파이썬이 못 읽을 수 있어 6자리로 줄인다.
    if "." in ts:
        head, tail = ts.split(".", 1)
        frac = "".join(ch for ch in tail if ch.isdigit())
        zone = tail[len(frac):]
        ts = f"{head}.{frac[:6]}{zone}"
    dt = datetime.fromisoformat(ts)
    if dt.tzinfo is None:
        from datetime import timezone
        dt = dt.replace(tzinfo=timezone.utc)
    return dt


def norm(p):
    return (p or "").replace("/", "\\").lower()


def main():
    truth = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
    out_dir = Path(sys.argv[2])
    summary_path = Path(sys.argv[3])
    evidence = json.loads((out_dir / "evidence.json").read_text(encoding="utf-8"))
    run = json.loads((out_dir / "run.json").read_text(encoding="utf-8"))
    events = evidence["events"]

    rows = []  # (구분, 항목, 판정, 설명)
    hard_fail = False

    def check(group, label, ok, detail, hard=True):
        nonlocal hard_fail
        verdict = "통과" if ok else ("실패" if hard else "참고")
        if hard and not ok:
            hard_fail = True
        rows.append((group, label, verdict, detail))

    def within(ev_time, t):
        dt = parse(ev_time)
        lo, hi = parse(t["time_min_utc"]) - SLACK, parse(t["time_max_utc"]) + SLACK
        return dt is not None and lo <= dt <= hi, f"{ev_time} (정답 범위 {lo.isoformat()} ~ {hi.isoformat()})"

    # --- 바로 가기 ---
    opened = [e for e in events if e["kind"] == "FileOpened"]
    for t in truth.get("opened", []):
        match = [e for e in opened if norm(e["subject"]) == norm(t["target"])]
        check("열람", t["label"], len(match) == 1,
              f"대상 {t['target']} → 찾은 기록 {len(match)}개 (만든 방법: {t['method']})")
        if not match:
            continue
        e = match[0]
        check("열람", t["label"] + " 볼륨 시리얼",
              e["details"].get("volume_serial") == t["volume_serial"],
              f"도구 {e['details'].get('volume_serial')} / 정답 {t['volume_serial']}")
        check("열람", t["label"] + " 크기",
              e["details"].get("target_size_bytes") == str(t["size"]),
              f"도구 {e['details'].get('target_size_bytes')} / 정답 {t['size']}")
        ok, detail = within(e["time"]["utc"], t)
        check("열람", t["label"] + " 시각", ok, detail)

    # --- 휴지통 ---
    deleted = [e for e in events if e["kind"] == "FileDeleted"]
    for t in truth.get("deleted", []):
        match = [e for e in deleted if norm(e["subject"]) == norm(t["original_path"])]
        check("삭제", t["label"], len(match) == 1,
              f"원래 경로 {t['original_path']} → 찾은 기록 {len(match)}개")
        if not match:
            continue
        e = match[0]
        check("삭제", t["label"] + " 크기", e["details"].get("size_bytes") == str(t["size"]),
              f"도구 {e['details'].get('size_bytes')} / 정답 {t['size']}")
        ok, detail = within(e["time"]["utc"], t)
        check("삭제", t["label"] + " 시각", ok, detail)
        check("삭제", t["label"] + " 계정", e["account"].get("sid") == truth["sid"],
              f"도구 {e['account'].get('sid')} / 정답 {truth['sid']}")

    # --- 브라우저 (참고) ---
    visits = [e for e in events if e["kind"] == "WebVisit"]
    for t in truth.get("web", []):
        match = [e for e in visits if e["subject"] == t["url"]]
        if match:
            ok, detail = within(match[0]["time"]["utc"], t)
            check("웹", t["browser"], ok, f"방문 기록 있음, 시각 {detail}", hard=False)
        else:
            check("웹", t["browser"], False,
                  "표식 주소의 방문 기록이 없음 (헤드리스 실행이 기록을 남기지 않았을 수 있음)",
                  hard=False)

    # --- 기타 ---
    check("기타", "입력 불변", run.get("input_unchanged") is True,
          f"input_unchanged={run.get('input_unchanged')} (실행 중 PC에서는 다른 프로그램 때문에 바뀔 수 있음)",
          hard=False)
    check("기타", "표시 시각 형식", "(" in run.get("finished_display", ""),
          run.get("finished_display", ""), hard=True)

    issues = evidence.get("issues", [])
    lines = [
        "## Securidge Trace — Windows 실기 검증",
        "",
        f"- OS: {truth['os'].get('Caption')} {truth['os'].get('Version')} (build {truth['os'].get('BuildNumber')})",
        f"- 시간대: {truth['time_zone']} / 계정: {truth['account']} ({truth['sid']})",
        f"- 뽑아낸 기록: {len(events)}개, 읽지 못한 항목: {len(issues)}개",
        f"- 보고서 생성 시각: {run.get('finished_display')}",
        "",
        "| 구분 | 항목 | 판정 | 설명 |",
        "|---|---|---|---|",
    ]
    for g, label, verdict, detail in rows:
        lines.append(f"| {g} | {label} | {verdict} | {detail.replace('|', '/')} |")
    if truth.get("notes"):
        lines += ["", "### 메모", *[f"- {n}" for n in truth["notes"]]]
    if issues:
        lines += ["", "### 읽지 못한 항목 (앞 20개)"]
        lines += [f"- `{i['file']}`: {i['reason']}" for i in issues[:20]]
    text = "\n".join(lines) + "\n"
    summary_path.write_text(text, encoding="utf-8")
    print(text)
    sys.exit(1 if hard_fail else 0)


if __name__ == "__main__":
    main()
