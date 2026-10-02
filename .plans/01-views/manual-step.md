# view: ManualStep (수동 개입 대기 — 모달 컴포넌트)

status: implemented (mock)

## 목적/진입
RunProgress 실행 중 폰 측 조작이 필요한 지점(§12)에서 모달로 표시. 별도 라우트 아님.

## 지점 정의 (plan.md §12)
| id | 안내 | 자동 감지 |
|---|---|---|
| mode-wait | 부트로더(fastboot) 모드 | usb_modes에서 fastboot 인터페이스 감지 → 자동 진행 |
| flash-mode | 플래시 모드(전원 끄고 볼륨 아래 + USB) | usb_modes에서 flashmode 감지 → 자동 진행 |
| ims-check | 최종 VoLTE 확인 | device_list 슬롯별 IMS 음성 등록(on) → 자동 진행 |
| (magisk-patch) | — | 사용하지 않음: 루팅은 자동 패치 |
| usb-debug | 폰에서 USB 디버깅 허용 | 단계 도달 시 같은 기기가 adb "device"면 안내 없이 진행, 아니면 안내 + 2초 폴링 → 연결되면 자동 진행 |
| su-grant | Magisk 루트 권한 허용 | `su -c id` 재시도 |
| magisk-patch | 폰의 Magisk 앱으로 부트 이미지 패치 | Download/magisk_patched*.img 신규 파일 감지 |
| oem-toggle | 개발자 옵션 OEM 잠금해제 켜기 | getvar 재시도 |
| mode-wait | 파란 LED(bootloader) 확인 | USB 모드 변경 감지 |
| ims-check | 최종 부팅 후 IMS 등록 확인 | dumpsys 질의(가능 시) |

## 인터랙션
- 단계별 안내 카드(번호 스텝) + [폰에서 완료했어요] 수동 ack 버튼
- 자동 감지 성공 시 모달 자동 해제 (mock: 타이머)


## 그림 안내 (사용자 지시 2026-10-03)

- 순서대로 따라 하는 조작은 카드 넘김(GuideSlides): 이전/다음 버튼, 점 표시, ←/→ 키, 카드 안 이미지 + 단계 제목/설명.
  한 화면에서 여러 항목을 보는 안내는 위아래 목록. 모달 내용이 길면 모달 안에서만 스크롤.
- 데이터: src/lib/data/guides.ts (ManualId별 슬라이드), 이미지: src/lib/assets/guide/
  - 폰 화면: 실기기(XQ-DQ44, Android 15) 스크린샷에서 필요한 부분만 잘라 사용 — 개인정보(IMEI·번호·주소 등) 영역 제외
  - 물리 조작(버튼·케이블): Codex(GPT 이미지 생성)로 제작, 지원 페이지풍 플랫 일러스트, 글자 없음
  - 강조 테두리는 이미지에 굽지 않고 테마 색(--primary)으로 CSS에서 그림 → 다크/라이트 공통
- 현재 적용: oem-toggle(빌드 번호 → 개발자 옵션 → OEM 잠금 해제 → USB 디버깅), flash-mode(일러스트 1장), usb-debug(빌드 번호 → USB 디버깅 → PC 연결·허용)
- 최종 VoLTE 확인(ims-check)은 앱이 IMS 등록으로 자동 판정하므로 그림 안내 없음
