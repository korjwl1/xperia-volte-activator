// 수동 개입 단계의 그림 안내 (순서대로 따라 하는 조작 → 카드 넘김)
// 스크린샷: XQ-DQ44 / Android 15 실기기에서 캡처 후 필요한 부분만 잘라 냄 (개인정보 없는 영역만)
// highlight: 이미지 기준 % 좌표 — 강조 테두리는 테마 색으로 CSS에서 그림 (다크/라이트 공통)
import type { ManualId } from "$lib/types";
import aboutBuildNumber from "$lib/assets/guide/about-build-number.png";
import devOptionsToggle from "$lib/assets/guide/dev-options-toggle.png";
import devOemUnlock from "$lib/assets/guide/dev-oem-unlock.png";
import devUsbDebugging from "$lib/assets/guide/dev-usb-debugging.png";
// 물리 조작 일러스트: Codex(GPT 이미지 생성)로 제작 — 글자·로고 없음
import flashMode from "$lib/assets/guide/flash-mode.jpg";
import usbConnect from "$lib/assets/guide/usb-connect.jpg";

export interface GuideSlide {
  image: string;
  /** 스크린샷(폰 화면) | 일러스트 */
  kind: "screen" | "illustration";
  title: string;
  caption: string;
  highlight?: { x: number; y: number; w: number; h: number };
  /** 강조 영역 옆 짧은 표시 (예: "7번 터치") */
  badge?: string;
}

export const GUIDES: Partial<Record<ManualId, GuideSlide[]>> = {
  "oem-toggle": [
    {
      image: aboutBuildNumber,
      kind: "screen",
      title: "개발자 옵션 열기",
      caption: "설정 > 휴대전화 정보 맨 아래 '빌드 번호'를 '개발자가 되었습니다'가 뜰 때까지 연속으로 터치합니다. 이미 켜져 있으면 건너뜁니다.",
      highlight: { x: 2, y: 52, w: 60, h: 44 },
      badge: "연속 터치",
    },
    {
      image: devOptionsToggle,
      kind: "screen",
      title: "개발자 옵션 확인",
      caption: "설정 > 시스템 > 개발자 옵션 맨 위 '개발자 옵션 사용'이 켜져 있는지 확인합니다.",
      highlight: { x: 77, y: 64, w: 16, h: 23 },
    },
    {
      image: devOemUnlock,
      kind: "screen",
      title: "OEM 잠금 해제 켜기",
      caption: "개발자 옵션에서 'OEM 잠금 해제'를 켭니다. 확인 창이 뜨면 '사용'을 누릅니다.",
      highlight: { x: 82, y: 30, w: 16, h: 42 },
    },
    {
      image: devUsbDebugging,
      kind: "screen",
      title: "USB 디버깅 켜기",
      caption: "같은 화면의 '디버깅' 항목에서 'USB 디버깅'을 켭니다. 다 켰으면 아래 [다시 확인]을 누릅니다.",
      highlight: { x: 82, y: 49, w: 16, h: 33 },
    },
  ],
  "flash-mode": [
    {
      image: flashMode,
      kind: "illustration",
      title: "볼륨 아래 버튼을 누른 채 USB 연결",
      caption: "폰 전원을 완전히 끄고 USB 케이블을 뺍니다. 볼륨 아래 버튼을 계속 누른 채 케이블을 꽂으면 플래시 모드로 들어갑니다. 앱이 감지하면 자동으로 진행되니 그때 손을 떼면 됩니다.",
    },
  ],
  "usb-debug": [
    {
      image: aboutBuildNumber,
      kind: "screen",
      title: "개발자 옵션 다시 열기",
      caption: "초기화 후에는 개발자 옵션이 꺼져 있습니다. 초기 설정을 마친 뒤 설정 > 휴대전화 정보의 '빌드 번호'를 연속으로 터치합니다.",
      highlight: { x: 2, y: 52, w: 60, h: 44 },
      badge: "연속 터치",
    },
    {
      image: devUsbDebugging,
      kind: "screen",
      title: "USB 디버깅 켜기",
      caption: "설정 > 시스템 > 개발자 옵션에서 'USB 디버깅'을 켭니다.",
      highlight: { x: 82, y: 49, w: 16, h: 33 },
    },
    {
      image: usbConnect,
      kind: "illustration",
      title: "PC에 연결하고 허용",
      caption: "USB로 PC에 연결하면 폰에 'USB 디버깅을 허용하시겠습니까?' 창이 뜹니다. '이 컴퓨터에서 항상 허용'을 체크하고 '허용'을 누르면 자동으로 진행됩니다.",
    },
  ],
};
