"""Windows 실기 검증 (2): 정답 하이브 대조, 이 PC 점검 대조.

사용
  python verify_more.py fake <fake-truth.json> <결과 폴더> <요약 파일>
  python verify_more.py live <live-truth.json> <결과 폴더> <요약 파일>
"""

import json
import sys
from pathlib import Path

rows = []
hard_fail = False


def check(group, label, ok, detail, hard=True):
    global hard_fail
    if hard and not ok:
        hard_fail = True
    verdict = ("통과" if hard else "참고") if ok else ("실패" if hard else "참고")
    rows.append((group, label, verdict, str(detail).replace("|", "/")))


def utc(s):
    return (s or "").replace("+00:00", "Z")


def fake(truth, case):
    a = case["analysis"]
    s = a["system"]
    check("시스템", "컴퓨터 이름", s.get("computer_name") == truth["computer_name"], s.get("computer_name"))
    check("시스템", "시간대", s.get("time_zone_key") == truth["time_zone"], s.get("time_zone_key"))
    check("시스템", "UTC 차이(분)", s.get("utc_offset_minutes") == truth["utc_offset_minutes"], s.get("utc_offset_minutes"))
    check("시스템", "코드 페이지", s.get("ansi_codepage") == truth["codepage"], s.get("ansi_codepage"))
    check("시스템", "Windows 이름·빌드", s.get("product_name") == truth["product"] and s.get("build") == truth["build"],
          f"{s.get('product_name')} {s.get('build')}")

    devs = a["devices"]
    u = truth["usb"]
    usb = [d for d in devs if d["kind"] == "UsbStorage" and d.get("serial") == u["serial"]]
    check("USB", "장치 찾기", len(usb) == 1, f"{len(usb)}개")
    if usb:
        d = usb[0]
        check("USB", "제조사·모델", (d.get("vendor"), d.get("product")) == (u["vendor"], u["product"]), f"{d.get('vendor')} / {d.get('product')}")
        check("USB", "고유 일련번호", d["serial_is_unique"] is True, d["serial_is_unique"])
        check("USB", "최초 연결", utc(d.get("first_install")) == u["first"], d.get("first_install"))
        check("USB", "마지막 연결", utc(d.get("last_arrival")) == u["last"], d.get("last_arrival"))
        check("USB", "마지막 분리", utc(d.get("last_removal")) == u["removed"], d.get("last_removal"))
        check("USB", "드라이브 문자", d.get("drive_letters") == u["letters"], d.get("drive_letters"))
    u2 = [d for d in devs if d.get("serial") == truth["usb2"]["serial"]]
    check("USB", "Windows가 만든 일련번호 표시", len(u2) == 1 and u2[0]["serial_is_unique"] is False,
          [d["serial_is_unique"] for d in u2])

    p = truth["portable"]
    port = [d for d in devs if d["kind"] == "Portable"]
    check("휴대 장치", "휴대 장치만 골라냄 (키보드 제외)", len(port) == 1, f"{len(port)}개")
    if port:
        d = port[0]
        check("휴대 장치", "제조사·모델·일련번호", (d.get("vendor"), d.get("product"), d.get("serial")) == (p["vendor"], p["product"], p["serial"]),
              f"{d.get('vendor')} / {d.get('product')} / {d.get('serial')}")
        check("휴대 장치", "마지막 연결", utc(d.get("last_arrival")) == p["last"], d.get("last_arrival"))

    b = truth["bluetooth"]
    bt = [d for d in devs if d["kind"] == "Bluetooth"]
    check("블루투스", "장치", len(bt) == 1 and bt[0].get("friendly_name") == b["name"] and bt[0].get("serial") == b["serial"],
          [(d.get("friendly_name"), d.get("serial")) for d in bt])
    if bt:
        check("블루투스", "마지막 연결", utc(bt[0].get("last_arrival")) == b["last"], bt[0].get("last_arrival"))

    n = truth["network"]
    nets = [x for x in a["networks"] if x["name"] == n["name"]]
    check("네트워크", "프로필", len(nets) == 1, f"{len(nets)}개")
    if nets:
        x = nets[0]
        check("네트워크", "무선 구분", x["medium"] == n["medium"], x["medium"])
        check("네트워크", "최초 연결(로컬→UTC)", utc(x.get("created_utc")) == n["created_utc"], f"{x.get('created_local_raw')} → {x.get('created_utc')}")
        check("네트워크", "마지막 연결(로컬→UTC)", utc(x.get("last_connected_utc")) == n["last_utc"], f"{x.get('last_connected_local_raw')} → {x.get('last_connected_utc')}")
        check("네트워크", "게이트웨이 MAC", x.get("gateway_mac") == n["gateway"], x.get("gateway_mac"))

    acc = [x for x in a["accounts"] if x["sid"] == truth["account"]["sid"]]
    check("계정", "SID ↔ 이름(한글)", len(acc) == 1 and acc[0]["name"] == truth["account"]["name"], [x["name"] for x in acc])

    kinds = {}
    for e in a["events"]:
        kinds[e["kind"]] = kinds.get(e["kind"], 0) + 1
    check("타임라인", "장치·네트워크 기록이 사실로 들어감",
          kinds.get("DeviceFirstConnected", 0) >= 1 and kinds.get("DeviceLastRemoved", 0) >= 1
          and kinds.get("NetworkFirstConnected", 0) == 1 and kinds.get("BluetoothConnected", 0) == 1, kinds)
    rules = sorted({f["rule_id"] for f in case["assessment"]["findings"]})
    check("판단", "처음 보는 저장장치(R1)·휴대 장치(R7) 발동", "R1" in rules and "R7" in rules, rules)


def live(truth, case):
    a = case["analysis"]
    s = a["system"]
    col = {c["artifact"]: c for c in case["collection"]}
    for art in ["hive_system", "hive_software", "evtx_security", "evtx_system"]:
        c = col.get(art, {})
        check("수집", art, c.get("ok") is True, c.get("method", "") + (" " + c.get("message", "") if not c.get("ok") else ""))
    for art in ["evtx_partition", "evtx_wlan"]:
        c = col.get(art, {})
        check("수집", art, c.get("ok") is True, c.get("message", "")[:120], hard=False)
    check("점검 정보", "관리자 권한으로 실행", case["info"]["admin"] is True, case["info"]["admin"])
    check("시스템", "컴퓨터 이름 = PowerShell 값", (s.get("computer_name") or "").upper() == truth["computer_name"].upper(),
          f"{s.get('computer_name')} / {truth['computer_name']}")
    check("시스템", "시간대 = PowerShell 값", s.get("time_zone_key") == truth["time_zone"], f"{s.get('time_zone_key')} / {truth['time_zone']}")
    check("시스템", "빌드 = PowerShell 값", s.get("build") == truth["build"], f"{s.get('build')} / {truth['build']}")
    acc = {x["sid"]: x["name"] for x in a["accounts"]}
    check("계정", "현재 계정 SID ↔ 이름", acc.get(truth["sid"], "").lower() == truth["account"].lower(), acc.get(truth["sid"]))
    ps_nets = sorted(truth["networks"])
    our_nets = sorted(x["name"] for x in a["networks"])
    check("네트워크", "프로필 이름 목록 = PowerShell 값", our_nets == ps_nets, f"{our_nets} / {ps_nets}")
    rules = [f for f in case["assessment"]["findings"]]
    r5 = [f for f in rules if f["rule_id"] == "R5"]
    check("판단", "로그 삭제(R5) — Security·System을 지움", len(r5) >= 2, [f["params"].get("log") for f in r5])
    r6 = [f for f in rules if f["rule_id"] == "R6"]
    check("판단", "사람이 바꾼 시계(R6)", len(r6) >= 1, [f["params"].get("minutes") for f in r6])
    check("결과", "등급 = 확인 필요", case["assessment"]["grade"] == "Review", case["assessment"]["grade"])
    # --- 새 흔적 (v0.2) ---
    ntuser = [c for c in case["collection"] if c["artifact"] == "hive_ntuser"]
    check("수집", "로그온 사용자 하이브 reg save HKU", any(c["ok"] for c in ntuser), [(c["method"], c["ok"], c["message"][:80]) for c in ntuser])
    for art in ["hive_usrclass", "hive_amcache", "srum", "evtx_rdp_session", "evtx_rdp_auth"]:
        cs = [c for c in case["collection"] if c["artifact"] == art]
        check("수집", art, any(c["ok"] for c in cs), [(c["method"][-40:], c["ok"], c["message"][:100]) for c in cs], hard=False)
    check("수집", "드라이브 정보 (C:의 장치 경로·일련번호)",
          any(d["letter"] == "C:" and d["device"].lower().startswith("\\device\\") and d.get("serial") for d in case["info"].get("drives", [])),
          [(d["letter"], d["device"], d.get("serial"), d["removable"]) for d in case["info"].get("drives", [])][:6])
    ev = a["events"]
    count = {}
    for e in ev:
        key = (e["kind"], e["source"]["artifact"])
        count[key] = count.get(key, 0) + 1
    for kind, art in [("ProgramRun", "prefetch"), ("ProgramRun", "bam"), ("ProgramRun", "userassist"), ("ProgramPresent", "hive_amcache"),
                      ("FolderOpened", "shellbags"), ("FileOpened", "jumplist"), ("NetworkUsage", "srum"), ("RemoteLogon", "evtx_security")]:
        check("새 흔적", f"{kind} ← {art}", count.get((kind, art), 0) > 0, f"{count.get((kind, art), 0)}건", hard=False)
    pf = truth.get("prefetch", {})
    runs = [e for e in ev if e["kind"] == "ProgramRun" and e["source"]["artifact"] == "prefetch" and e["subject"].upper().endswith("\\" + pf.get("name", "?"))]
    if pf.get("exists"):
        check("Prefetch", "직접 실행한 프로그램 찾기", len(runs) >= 1, [r["subject"] for r in runs][:2])
        if runs:
            rc = {r["details"].get("run_count") for r in runs}
            check("Prefetch", "실행 횟수 = 3", rc == {str(pf["runs"])}, rc)
            latest = max(r["time"]["utc"] for r in runs)
            from datetime import datetime, timedelta
            p = lambda x: datetime.fromisoformat(x.replace("Z", "+00:00"))
            ok = p(pf["start"]) - timedelta(seconds=5) <= p(latest) <= p(pf["end"]) + timedelta(seconds=120)
            check("Prefetch", "마지막 실행 시각이 실행한 구간 안", ok, f"{latest} / {pf['start']}~{pf['end']}")
            check("Prefetch", "실행 시각 개수 = 3", len(runs) == pf["runs"], len(runs))
    else:
        check("Prefetch", "이 VM에서 Prefetch 파일이 만들어짐", False, "SysMain이 파일을 만들지 않음", hard=False)
    bad = [i for i in a["issues"] if i["artifact"] in ("prefetch", "jumplist", "hive_ntuser", "hive_usrclass", "hive_amcache", "srum")
           and not i["reason"].startswith("참고:")]
    check("새 흔적", "읽지 못한 파일", len(bad) == 0, [(i["artifact"], i["file"][-50:], i["reason"][:80]) for i in bad][:5], hard=False)

    tool = case["info"]["tool_media"]
    check("점검 도구", "도구 매체 식별 (D: 드라이브에서 실행)", (tool.get("drive_letter") or "").upper() == "D:" and bool(tool.get("volume_serial")),
          tool, hard=False)


def main():
    mode, truth_path, out_dir, summary = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    truth = json.loads(truth_path.read_text(encoding="utf-8-sig"))
    case = json.loads((out_dir / "case.json").read_text(encoding="utf-8"))
    (fake if mode == "fake" else live)(truth, case)
    title = "정답 하이브 대조" if mode == "fake" else "이 PC 점검 실기"
    lines = [f"## Securidge Trace — {title}", "", "| 구분 | 항목 | 판정 | 설명 |", "|---|---|---|---|"]
    lines += [f"| {g} | {l} | {v} | {d} |" for g, l, v, d in rows]
    text = "\n".join(lines) + "\n"
    summary.write_text(text, encoding="utf-8")
    print(text)
    sys.exit(1 if hard_fail else 0)


if __name__ == "__main__":
    main()
