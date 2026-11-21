# Securidge Trace

Windows PC에 남은 사용 기록을 모아 **누구나 한눈에 이해할 수 있는 보고서**로 만드는 점검 도구.
연결됐던 USB·스마트폰·블루투스, 연결했던 네트워크, 열어 본 파일, 지운 파일, 웹 방문·다운로드,
로그 삭제·시계 변경을 읽고, 판단 규칙에 따라 "특이사항 미발견 / 주의 / 확인 필요 / 판단 불가"로 정리한다.

대상 사용자: IT 지식이 없어도 되는 **유지보수 업체 직원**. 설치 없이 USB에서 실행한다. 한국어·영어.

## 내려받기

[Releases](../../releases) 에서 `SecuridgeTrace-<버전>-windows-x64.zip`을 받아 압축을 풀고
`SecuridgeTrace.exe`를 실행한다. 사용법은 zip 안의 `사용안내.txt` (영어: `Guide-EN.txt`).

- 변조 확인: zip과 함께 올린 `.sha256` 파일의 값과 비교한다 (`Get-FileHash 파일.zip -Algorithm SHA256`).
- 코드 서명을 하지 않았으므로 처음 실행할 때 "Windows의 PC 보호" 창이 뜰 수 있다. [추가 정보] → [실행].

## 용도와 한계

- **1차 점검·참고용이다. 법정 증거용이 아니다.** 판단은 가능성을 알려줄 뿐 특정인의 행동을 단정하지 않는다.
- **점검할 권한이 있는 PC에서만 사용한다** (본인 소유이거나 계약·동의로 권한을 받은 PC).
  직원 PC를 점검할 때는 고객사의 법무·개인정보 담당자의 검토가 필요하다.
- 결과(지난 내역)에는 개인정보가 들어 있다. 보관과 삭제는 사용하는 쪽의 책임이다. v0.1은 내역 암호 보호가 없다.
- "기록이 없다"는 "행위가 없었다"는 뜻이 아니다.
- 확인하지 못하는 것: 클라우드 동기화·메신저·메일로 보낸 파일, 인쇄, 화면 촬영, 열지 않고 복사만 한 파일.

## 무엇을 읽는가

| 자료 | 읽는 것 | 방법 |
|---|---|---|
| SYSTEM 하이브 | 컴퓨터 이름, 시간대, 코드 페이지, USB 저장장치(최초·마지막 연결, 분리), 휴대 장치(MTP/PTP), 블루투스, 드라이브 문자 | 이 PC 점검: `reg save`, 트랜잭션 로그 적용 |
| SOFTWARE 하이브 | Windows 버전, 계정(SID ↔ 이름), 네트워크(유선·무선, 최초·마지막 연결, 게이트웨이 MAC) | 〃 |
| 이벤트 로그 | 로그 삭제(1102·104), 시계 변경(4616·Kernel-General 1), 저장장치 연결(1006, 볼륨 일련번호), 무선 연결(8001·8003) | `wevtutil epl` |
| 바로 가기(LNK) | 열어 본 파일, 볼륨 일련번호, 드라이브 종류 | 직접 읽기 |
| 휴지통 `$I` | 지운 파일의 원래 경로·크기·시각·계정 | 직접 읽기 |
| 브라우저 | Chrome·Edge·Whale 방문·다운로드, Firefox 방문 | 사본을 만들어 읽기 |

판단 규칙: R1 처음 보는 저장장치, R2 업무 시간 외 저장장치, R3 이동식 저장장치의 파일 열람(볼륨 일련번호로 장치와 연결),
R4 대량 삭제, R5 로그 삭제, R6 사람이 바꾼 시계, R7 휴대 장치, R8 클라우드·웹메일 접속.
회사 장치 목록에 있는 장치와 점검 도구가 담긴 USB는 판단에서 뺀다. 기준값은 설정에서 바꿀 수 있다.

## 검증 현황

GitHub Actions의 Windows 가상 머신(Windows Server 2022, 2025)에서 매 push마다 자동으로 검증한다.
결과는 Actions 실행 화면의 요약(Summary)에 표로 나온다.

| 검증 | 방법 | Server 2022 / 2025 | 일반 PC용 Windows 10 / 11 |
|---|---|---|---|
| 바로 가기: 경로(한글), 볼륨 일련번호, 크기, 열람 시각 | 셸 API로 만든 실제 바로 가기와 대조 + 공개 샘플 17개 교차 검증 | 통과 | 미검증 |
| 휴지통: 경로(한글), 크기, 삭제 시각, 계정 | 실제로 휴지통에 보낸 파일과 대조 | 통과 | 미검증 |
| 레지스트리 파서: USB·휴대 장치·블루투스·드라이브 문자·네트워크(로컬→UTC)·계정 | Windows에서 `reg save`로 만든 **정답 하이브**와 대조 | CI 결과 참고 | 미검증 |
| 이 PC 점검: 수집, 시스템 정보·계정·네트워크를 PowerShell 값과 대조, 로그 삭제(R5)·시계 변경(R6) 탐지 | 실제로 로그를 지우고 시계를 바꾼 뒤 `trace check` | CI 결과 참고 | 미검증 |
| 화면 프로그램 실행 | 실행 후 15초 생존, 스크린샷 | CI 결과 참고 | 미검증 |
| 실제 USB 장치·스마트폰 | — | 불가 (가상 머신에 장치 없음) | 미검증 |
| 브라우저 실제 DB | — | 미검증 (헤드리스 실행이 기록을 남기지 않음) | 미검증 |
| 판단 규칙, 시각 표기, 보고서, 손상 입력 내성 | 단위·통합 테스트 | 통과 | — |

**실제 USB와 일반 PC용 Windows 10·11, 화면의 한글 입력은 사람이 한 번 확인해야 한다.**
절차: `docs/test-scenarios.md`

## 직접 빌드

```
cargo test --workspace
cargo build --release -p st-gui -p st-cli
```
Rust 1.97.0 (`rust-toolchain.toml`). 결과: `target/release/SecuridgeTrace.exe`(화면), `trace.exe`(명령줄).

명령줄:
```
trace check   --i-am-authorized --customer "고객사A" --analyst "홍길동" [--days 30] [--lang ko|en]
trace analyze --input E:\ --output D:\결과 --case "PC-01"
trace regen   --dir <내역 폴더> --lang en
```
종료 코드: 0 정상 / 2 끝났지만 읽지 못한 항목이 있거나 입력이 바뀜 / 1 실행하지 못함

## 폴더 구성

```
crates/core      데이터 모델, 설정, 시각 변환, 해시, 안전한 바이트 읽기
crates/i18n      한국어·영어 문구(tools/locales.py에서 생성), 시각 표기
crates/collect   입력에서 기록 파일 찾기
crates/parsers   하이브·이벤트 로그·바로 가기·휴지통·브라우저 파서
crates/timeline  사실로 바꾸고 시간순으로 합치기, 계정 연결
crates/rules     상관 분석, 판단 규칙, 등급
crates/report    case.json·evidence.json·timeline.csv·HTML 보고서
crates/engine    점검 실행(이 PC 점검 수집 포함), 설정·고객사·지난 내역
crates/gui       화면 프로그램 (egui)
crates/cli       명령줄
ci/              Windows 실기 검증 스크립트
docs/            Phase 0 보고서, 결정 기록, 데이터 모델, 화면 설계, 시험 절차서
dist/            배포물에 넣는 안내문
```

## 라이선스

MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). 포함된 외부 라이브러리 목록은 배포 zip의 `THIRD-PARTY-LICENSES.txt`.
시험용 바로 가기 샘플은 github.com/AbdulRhmanAlfaifi/lnk_parser(MIT)에서 가져왔다.
