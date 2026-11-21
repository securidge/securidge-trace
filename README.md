# Securidge Trace 

Windows PC에 남은 사용자 행위 기록(연결된 장치, 열어 본 파일, 지운 파일, 웹 기록 등)을 모아
한눈에 볼 수 있는 보고서로 만드는 점검 도구.

> **개발 중.** 지금은 명령줄에서 "수집해 둔 자료"의 휴지통·바로 가기·브라우저 기록만 읽는다.
> 화면(GUI), USB·네트워크 기록, 실행 중인 PC 점검, HTML 보고서는 아직 없다.

## 용도와 한계

- **1차 점검·참고용이다. 법정 증거용이 아니다.**
- **점검할 권한이 있는 PC에서만 사용한다.** (본인 소유이거나, 계약·동의로 권한을 받은 PC)
- 직원 PC를 점검할 때는 고객사의 법무·개인정보 담당자의 검토가 필요하다.
- 결과에는 개인정보가 들어 있다. 보관과 삭제는 사용하는 쪽의 책임이다.
- "기록이 없다"는 "행위가 없었다"는 뜻이 아니다. 기록이 지워졌거나 남지 않는 방식일 수 있다.

## 빌드와 실행

```
cargo test --workspace
cargo build --release -p st-cli

trace analyze --input <입력 폴더> --output <결과 폴더> --case "고객사A PC-01" --analyst "홍길동"
```

- 입력: Windows의 원래 폴더 구조를 유지한 폴더, 또는 읽기 전용으로 연결한 디스크의 드라이브 경로
- 옵션: `--lang ko|en`, `--tz Asia/Seoul`, `--ansi-codepage 949|1252`
- 종료 코드: 0 정상 / 2 끝났지만 읽지 못한 항목 있음 / 1 실행하지 못함
- Windows 실행 파일은 GitHub Actions(`.github/workflows/ci.yml`)가 만든다.

## 검증 현황

| 항목 | 검증 방법 | Windows 10 | Windows 11 |
|---|---|---|---|
| 시각 변환, 시각 표기, 해시 | 알려진 값으로 단위 테스트 | 해당 없음 | 해당 없음 |
| 바로 가기(LNK): 대상 경로, 볼륨 시리얼, 드라이브 종류, 대상 시각 | 실제 Windows 샘플 17개 + 다른 파서와 교차 검증 | **검증됨** (고정 디스크·네트워크 공유) | 미검증 |
| 바로 가기: 이동식 드라이브(USB), 한글 경로, 코드 페이지 949 | - | 미검증 | 미검증 |
| 휴지통 `$I` | 형식 문서 기준 합성 데이터 | 미검증 (실제 파일 없음) | 미검증 |
| 브라우저 기록 (Chrome·Edge·Whale·Firefox) | 합성 DB | 미검증 (실제 DB 없음) | 미검증 |
| 브라우저 경로 (`config/browsers.json`) | - | 미검증 | 미검증 |
| 손상된 입력에서 멈추지 않음 | 자르기·바이트 바꾸기 간이 시험 (정식 퍼징 아님) | - | - |
| 분석 후 입력 불변, 같은 입력이면 같은 결과 | 자동 테스트 | Linux에서만 확인 | Linux에서만 확인 |
| Windows에서 실행 | - | **미검증** | **미검증** |

실제 데이터로 검증하려면 `docs/test-scenarios.md`의 절차로 시험 데이터를 만들어야 한다.

## 폴더 구성

```
crates/core      공통 데이터 모델, 시각 변환, 해시, 안전한 바이트 읽기
crates/i18n      한국어·영어 문구, 시각 표기
crates/collect   입력에서 기록 파일 찾기
crates/parsers   기록 파일 해석 (휴지통, 바로 가기, 브라우저)
crates/timeline  사실로 바꾸고 시간순으로 합치기
crates/report    결과 파일 쓰기
crates/cli       명령줄 실행 파일
config/          브라우저 경로 목록
docs/            Phase 0 보고서, 데이터 모델, 화면 설계안, 시험 절차서
```

## 시험용 샘플의 출처

`crates/parsers/tests/data/lnk/`의 바로 가기 샘플은 github.com/AbdulRhmanAlfaifi/lnk_parser 저장소(MIT)에서 가져왔다.
라이선스 전문은 같은 폴더의 `LICENSE-samples`에 있다.

## 라이선스

MIT OR Apache-2.0
