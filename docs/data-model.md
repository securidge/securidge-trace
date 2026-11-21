# 데이터 모델 (초안)

## 1. 지금 구현된 것 (`crates/core/src/model.rs`)

| 이름 | 뜻 | 주요 항목 |
|---|---|---|
| `Stamp` | 시각 하나 | `utc`(UTC 값), `raw`(원본 값), `basis`(Utc / Local / FileSystem) |
| `Event` | 파서가 뽑은 **사실** 하나 | `time`, `kind`, `subject`(파일 경로·URL), `account`, `details`, `source`, `confidence`, `caveats` |
| `EventKind` | 사실의 종류 | FileDeleted, FileOpened, WebVisit, WebDownload (단계마다 추가) |
| `SourceRef` | 출처 | `artifact`, `file`(입력 기준 상대 경로), `record`(파일 안 위치) |
| `AccountRef` | Windows 계정 | `sid`, `name` (아는 것만) |
| `Confidence` | 신뢰도 | High / Medium / Low |
| `Issue` | 읽지 못한 항목 | `artifact`, `file`, `reason` |
| `Coverage` | 아티팩트별 데이터 범위 | `files`, `events`, `earliest`, `latest` |

원칙
- **사실과 판단을 분리한다.** `Event`에는 판단이 없다. 규칙의 결과는 별도 구조(아래 `Finding`)로 둔다.
- 시각은 UTC로 보관하고 원본 값과 기준을 함께 남긴다. `FileSystem` 기준은 "수집 과정에서 바뀌었을 수 있는 값"이라는 뜻이다.
- 출력 순서가 실행마다 같도록 정렬 가능한 구조(`BTreeMap`)만 쓴다.

## 2. 앞으로 추가할 것

| 이름 | 단계 | 뜻 |
|---|---|---|
| `SystemInfo` | Phase 2 | 컴퓨터 이름, Windows 버전·빌드, 설치 일시, 시간대, ANSI 코드 페이지, 마지막 종료 시각 |
| `Account` | Phase 2 | SID ↔ 계정명 ↔ 프로필 경로 (지금은 폴더 이름과 SID가 따로 논다) |
| `Device` | Phase 2 | 저장장치·스마트폰·블루투스 장치. 제조사, 모델, 장치 시리얼, 볼륨 시리얼, 최초·최종 연결, 제거 시각, 각 값의 출처 |
| `Network` | Phase 2 | 유선/무선 구분, 이름, 게이트웨이 MAC, 최초·최종 연결 |
| `CollectionManifest` | Phase 3 | 수집한 파일마다 원래 경로, **원래 파일 시각(생성·수정·접근)**, 크기, 해시, 읽은 방법 |
| `Link` | Phase 4 | 두 사실 사이의 연결과 강도: Direct(볼륨 시리얼 일치) / Circumstantial(드라이브 문자·시각) |
| `Incident` | Phase 4 | 연결된 사실들의 묶음 (장치 연결 → 열람 → 삭제 → 제거) |
| `Finding` | Phase 4 | 규칙이 내린 **판단**: 규칙 ID, 등급, 근거 사실 목록, 쉬운 설명 |
| `Case` | Phase 5 | 점검 한 건: 고객사, PC, 점검자, 점검 기간, 시작·종료 시각, 권한 확인 여부 |

## 3. 결과 파일 (지금)

```
Trace_<사건명>_<YYYYMMDD_HHMMSS>/
  evidence.json    근거 전체. 같은 입력이면 항상 같은 내용 (실행 시각·절대 경로 없음)
  timeline.csv     시간순 목록 (엑셀용, UTF-8 BOM)
  run.json         이번 실행 정보 (시각, 점검자, 언어, 입력 불변 여부)
  SHA256SUMS.txt   위 파일들의 해시
```

`evidence.json`에는 `schema_version`이 있다. 구조를 바꾸면 이 숫자를 올리고, 지난 내역을 열 때 이 값을 보고 변환한다.

## 4. 지난 내역 저장 형식 (계획, Phase 5)

```
data/
  settings.toml               기본 설정
  customers/<고객사>.toml      고객사 프로필 (업무 시간, 회사 장치 목록)
  history/
    index.json                목록 화면용 요약 (일시, 고객사, PC, 점검자, 등급)
    <YYYYMMDD_HHMMSS>_<PC>/
      case.json  evidence.json  report_ko.html  report_en.html
      run.log  rules.toml(사용한 규칙 사본)  SHA256SUMS.txt
      collected/              수집본 (보관을 켠 경우만)
```

- 다른 언어로 보고서를 다시 만들 때는 `evidence.json`만 읽는다 (재수집 없음).
- 목록(`index.json`)이 깨져도 각 폴더의 `case.json`으로 다시 만들 수 있게 한다.
