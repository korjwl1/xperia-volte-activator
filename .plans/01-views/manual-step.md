# view: ManualStep (수동 개입 대기 — 모달 컴포넌트)

status: implemented (mock)

## 목적/진입
RunProgress 실행 중 폰 측 조작이 필요한 지점(§12)에서 모달로 표시. 별도 라우트 아님.

## 지점 정의 (plan.md §12)
| id | 안내 | 자동 감지 |
|---|---|---|
| usb-debug | 폰에서 USB 디버깅 허용 | `device:changed` + adb 승인 |
| su-grant | Magisk 루트 권한 허용 | `su -c id` 재시도 |
| magisk-patch | 폰의 Magisk 앱으로 부트 이미지 패치 | Download/magisk_patched*.img 신규 파일 감지 |
| oem-toggle | 개발자 옵션 OEM 잠금해제 켜기 | getvar 재시도 |
| mode-wait | 파란 LED(bootloader) 확인 | USB 모드 변경 감지 |
| ims-check | 최종 부팅 후 IMS 등록 확인 | dumpsys 질의(가능 시) |

## 인터랙션
- 단계별 안내 카드(번호 스텝) + [폰에서 완료했어요] 수동 ack 버튼
- 자동 감지 성공 시 모달 자동 해제 (mock: 타이머)
