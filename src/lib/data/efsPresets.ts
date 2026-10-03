// EFS 프리셋 manifest — 원본 beta11 번들(util/SonyEFS, balance)의 출처·버전·해시
// 생성: Config.json efs.balance 경로의 폴더를 (상대 경로 + 내용) 순서대로 SHA-256 (2026-10-03)
// 프리셋 이름("for V" 등)이 적용 가능 기종을 보장하지 않는다 — 검증된 기종은 실제 패치 결과로만 추가한다.
import type { CarrierId } from "$lib/types";

// 데이터(EFS) 버전은 도구 버전과 별개 — 도구 beta9~beta11은 모두 이 세트를 담고 있음 (tasks/research-efs-preset-versions.md, 2026-10-03)
export const EFS_PRESET_VERSION = "20250901"; // Config.json efs.patchVer — 조사 시점 확인된 최신 공통 세트
export const EFS_PRESET_MODE = "balance";
/** 원본 배포 — 카페 재첨부 ZIP과 번들 ZIP의 SHA-256이 같음 */
export const EFS_PRESET_SOURCE = {
  url: "https://cafe.naver.com/x1smart/612141",
  archive: "SonyEFS 균형_기본_최종 적용[patched-20250901].zip",
  archiveSha256: "ee0265c39bab5c44269ffd611e25b97963a182a7420a61f880e0b79db4f05bc0",
  checked: "2026-10-03",
};
// performance(실내 우선) 세트는 쓰지 않는다 — Config.json의 LGU 경로 오타(_perf↔_peref), KT SIM1 접미사 5개,
// LGU SIM1에 프로필 모음 323개 포함 등 배포 구성 문제가 확인됨 (원본과 같으므로 자동 교정하지 않음)

export interface EfsPreset {
  carrier: CarrierId;
  slot: 1 | 2;
  folder: string;
  files: number;
  sha256: string;
}

export const EFS_PRESETS: EfsPreset[] = [
  { carrier: "SKT", slot: 1, folder: "./util/SonyEFS/XPERIA-SKT/XPERIAsSKT1", files: 82, sha256: "35e67f181a95f0126577304405575169321d6366db1d098dd320d8c59cb9d0a6" },
  { carrier: "SKT", slot: 2, folder: "./util/SonyEFS/XPERIA-SKT/XPERIAsSKT2", files: 82, sha256: "82ffa5d50d3e6140c038724163ebfb1b4dbf27f9d2806d08a4cc674ba85dc04d" },
  { carrier: "KT", slot: 1, folder: "./util/SonyEFS/XPERIA-KT/XPERIAsonyKT1", files: 116, sha256: "e8262cb4e56fbb0ed18fb17646f9b91b8d14fd4aefdeae6ffe38b34b778be791" },
  { carrier: "KT", slot: 2, folder: "./util/SonyEFS/XPERIA-KT/XPERIAsonyKT2", files: 114, sha256: "98cdf9244bf9daecbbb55e0219fbc08ed3b7a248017ae951851dfa790077b11e" },
  { carrier: "LGU", slot: 1, folder: "./util/SonyEFS/XPERIA-LGU/XPERIAsonyLGU1", files: 101, sha256: "ac3a078c07f6f38e8223fc96f4ec8598bdd3a2d45f14557c3ff232b84da8e2e9" },
  { carrier: "LGU", slot: 2, folder: "./util/SonyEFS/XPERIA-LGU/XPERIAsonyLGU2", files: 96, sha256: "eb24e89fda86eb59ced1640ec093c8fe9e77969dcbde199ae5d63f20fc0c22d7" },
  { carrier: "LGU_V", slot: 1, folder: "./util/SonyEFS/XPERIA-LGU/LGU1＿for＿V＿1st", files: 67, sha256: "c328be4fdede00810a6a68d4e8bf264f2b9d6ed1d80228499041c75dedc2f2a0" },
  { carrier: "LGU_V", slot: 2, folder: "./util/SonyEFS/XPERIA-LGU/LGU1＿for＿V＿1st_Subscription01", files: 65, sha256: "bda0578be86af85d6403e3f7da6d653c5f0c6bae0f4f53c1d306ade71af92a2a" },
];

/** 이전 성공 후보 (비교·수동 선택용, 자동 롤백 금지) — 2025-08 KT 5G·LGU 타사 발신 문제를 beta7 세트로 되돌려 해결한 보고가 있음.
 *  현재 세트의 결함을 입증하는 근거는 아니며, Pro-I KT 사례는 성공 폴더 해시가 미상이라 넣지 않음 */
export const EFS_PREVIOUS: { carrier: CarrierId; slot: 1 | 2; version: string; files: number; sha256: string }[] = [
  { carrier: "KT", slot: 1, version: "beta7_20250528", files: 116, sha256: "fc695ab09c30d5e6f6047645f0e9c458bb0d4b254dd46450da75582a86c14c7f" },
  { carrier: "KT", slot: 2, version: "beta7_20250528", files: 114, sha256: "d597d7c0ec126dcf6ea0a932ad1d760ea3078ebe645fb2d8881d4240be7a340f" },
  { carrier: "LGU", slot: 1, version: "beta7_20250528", files: 98, sha256: "b12e139bff695e7c3d191cae608b91a6e1149f27a9d8a6643a795eb906ff4e26" },
  { carrier: "LGU", slot: 2, version: "beta7_20250528", files: 99, sha256: "7e6a04057f7b484ea0e6aeced43787cd540d7d13d68d0b66061353ffe0f8fcd3" },
];

/** 프리셋 갱신 판정은 데이터 해시로 — 새 도구가 나와도 EFS 해시가 같으면 갱신이 아니다.
 *  새 세트는 격리 다운로드 → 파일 차이·XML 문법·슬롯 경로 검토 → 해당 기종 실물 확인 → manifest 승격 순서 */

/** 이 앱으로 패치가 확인된 기종 (모델·지역·Android·망·프리셋 해시별로 기록) — 아직 없음 */
export const EFS_VERIFIED: { model: string; android: string; carrier: CarrierId; sha256: string; date: string }[] = [];

/** 프리셋별 참고 — 카페 보고 (미검증) */
export const EFS_PRESET_NOTES: Partial<Record<CarrierId, string>> = {
  LGU_V: "1 V·5 V에 자동 적용. 10 VII에서 일반 LGU 프리셋 대신 필요했다는 사용자 보고 1건이 있으나 미검증",
};

export function efsPreset(carrier: CarrierId, slot: 1 | 2): EfsPreset | undefined {
  return EFS_PRESETS.find((p) => p.carrier === carrier && p.slot === slot);
}
