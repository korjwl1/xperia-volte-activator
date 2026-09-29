# view: BackupSelect (③ 백업 항목 선택 — "작업 옵션 선택" 좌측 백업 탭)

status: implemented (mock + 실측 용량)

## 목적/진입
초기화 루트가 있는 계획에서만 노출. 4 카테고리 · 아이템 단위 체크박스 제공. 기본 전체 체크(초기화 시).

## 상태 필드
- 카테고리 4종: [설정] [앱] [파일] [통화 및 문자] — `backupCategories`가 groupIds로 그룹 매핑
- 그룹/항목 구조 `mockBackupGroups: BackupGroup[]`:
  - 설정: 전체 설정 백업 1항목 (덤프 전체 + 화이트리스트 복원)
  - 앱: APK 파일 (pm path→stat 실측) + 앱 데이터 (Android/data 실측)
  - 파일: 표준 안드로이드 폴더 7개(DCIM/Download/Pictures/Movies/Music/Documents/Recordings)
    + "그 외 전체 파일 시스템"
    (그 외 = sdcard 전체 − Android/data − 기명 항목 합계 — Audiobooks/Podcasts/Ringtones/Alarms 등
     기기 특화 폴더도 여기에 포함)
  - 통화 및 문자: 통화 기록 → 문자 → 연락처
    (연락처는 구글 동기화/SIM 저장 여부와 무관하게 기기 내 연락처 DB를 백업 대상에 포함)
- 체크 상태는 **항목 단위** (`item.checked`) — 카테고리 헤더의 "전체 해제"는 하위 일괄 토글
- `sizesLoading` — 실측 용량 도착 전까지 항목 우측에 스켈레톤(animate-pulse),
  상단 sticky "예상 X GB"도 스켈레톤. 도착 시 `realSizes`로 바이트 갱신

## 인터랙션 → 계약 매핑
| 요소 | 동작 | 계약 |
|---|---|---|
| 카테고리 전체 토글 | 하위 항목 일괄 | (프론트 로직) |
| 개별 체크 | 백업 대상 Set 갱신 → 예상 용량 재계산 | (프론트 로직) |
| [폴더 지정] | File System Access API 폴더 선택 → 경로 표시 | (프론트, 데스크톱에서는 추후 dialog 플러그인) |
| 용량 실측 | 진입 시 1회 | `storage_sizes` (✅ Rust 구현, 브라우저 dev/실패 시 → {}) |
| [실행] | 방어(경로 미지정·공간 부족 → 4초 자동 해제 알림) 후 ③으로 | `wizard.groups`에 항목 단위 checked 스냅샷 |
| [뒤로] | ② 복귀 | — |

## 특수 규칙
- 여유 공간 85% 초과 시 적색 경고 + 실행 차단(알림)
- "그 외 전체 파일 시스템"은 기본 미체크 (전체 백업 시에만 체크)
- 우측 "실행 순서" 미리보기: 언락/리락 단계 ⚠ 툴팁(초기화 안내)

## 비주얼 (desktop-ui 스킬)
좌측 탭(백업/루팅) + sticky 경로 바(용량/여유) + 카테고리 헤더(선택 수 + 전체 토글) /
항목 카드 체크 시 primary 톤, 우측 용량은 font-mono · 로딩 시 스켈레톤.
