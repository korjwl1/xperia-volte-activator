// 백업 항목 정의 — 앱별 복원 분류는 data/appRules.ts
import type { BackupGroup } from "$lib/types";

const MB = 1024 ** 2;

// 백업 항목 — 용량은 storage_sizes 실측(PlanReviewView에서 항목 id로 매핑).
// estBytes는 실측 불가 항목(텍스트 덤프)의 고정 추정치이며 UI에 "추정"으로 표시한다.
export const mockBackupGroups: BackupGroup[] = [
  {
    id: "settings",
    label: "설정",
    desc: "백업 가능한 설정 항목",
    items: [
      { id: "settings-all", label: "전체 설정 백업", cls: "full", checked: true, estBytes: 2 * MB },
    ],
  },
  {
    id: "apps",
    label: "앱",
    desc: "APK 추출 + 앱 데이터",
    items: [
      { id: "apk", label: "APK 파일", cls: "full", checked: true },
      { id: "app-data", label: "앱 데이터", cls: "full", checked: true },
    ],
  },
  {
    id: "files",
    label: "파일",
    desc: "사진, 동영상, 문서 등",
    items: [
      { id: "dcim", label: "사진·영상 (DCIM)", cls: "full", checked: true },
      { id: "download", label: "다운로드", cls: "full", checked: true },
      { id: "pictures", label: "Pictures", cls: "full", checked: true },
      { id: "movies", label: "Movies", cls: "full", checked: true },
      { id: "music", label: "Music", cls: "full", checked: true },
      { id: "documents", label: "Documents", cls: "full", checked: true },
      { id: "recordings", label: "Recordings", cls: "full", checked: true },
      { id: "fs-rest", label: "그 외 전체 파일 시스템", cls: "full", checked: true },
    ],
  },
  {
    id: "sms",
    label: "통화 및 문자",
    desc: "통화 기록, 문자 메시지, 연락처",
    items: [
      { id: "calllog", label: "통화 기록", cls: "full", checked: true, estBytes: 1 * MB },
      { id: "sms", label: "문자", cls: "full", checked: true, estBytes: 4 * MB },
      // 구글 동기화/SIM 저장과 무관하게 기기 내 연락처 DB도 백업 대상에 포함
      { id: "contacts", label: "연락처", cls: "full", checked: true, estBytes: 2 * MB },
    ],
  },
];
