# view: ModeSelect — 시작 종류

status: implemented (2026-10-08)

startSession 직후 A 자동 진행 | B 수동 진행 | C 업데이트를 한 행 3열로 표시한다. 사이드바 없이 헤더·이전 버튼을 사용한다. 상태는 device/opts.mode/checking/error다.

SelectionDeviceStatus는 deviceList로 같은 모델·시리얼의 승인된 Android 한 대를 3초마다 조회하며 클릭 직전 조회 성공 후 chooseMode한다. 실패·교체·조회 중은 비활성. 업데이트는 cellularReady SIM 하나 이상, 실패/모순/Wi-Fi만 등록은 불가. 카드 자체에 루팅/언락 조건은 없다.

자동→warning, 수동→manual-tasks, 업데이트→step1의 UpdateConfig, 이전→device. 전이는 기존 입력/작업/백업 위치를 정리하고 진행 중 I/O를 막는다. 새 쓰기 없음.

비주얼: 1280×720에서 동일 행 3카드, 왼쪽 정렬 lucide 아이콘·제목·설명, MD3 기본/비활성 톤, pane 스크롤과 시스템 테마.
