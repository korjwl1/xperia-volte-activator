# view: ModeSelect — 시작 종류

status: implemented (2026-10-08)

startSession 직후 A 자동 진행 | B 수동 진행 | C 업데이트를 한 행 3열로 표시한다. 사이드바 없이 헤더·이전 버튼을 사용한다. 상태는 device/opts.mode/checking/error다.

SelectionDeviceStatus는 deviceList로 같은 모델·시리얼의 승인된 Android 한 대를 3초마다 조회하며 클릭 직전 조회 성공 후 chooseMode한다. 실패·교체·조회 중은 비활성. 업데이트는 cellularReady SIM 하나 이상, 실패/모순/Wi-Fi만 등록은 불가. 카드 자체에 루팅/언락 조건은 없다.

자동→warning, 수동→manual-tasks, 업데이트→step1의 UpdateConfig, 이전→device. 전이는 기존 입력/작업/백업 위치를 정리하고 진행 중 I/O를 막는다. 새 쓰기 없음.

비주얼: 1280×720에서 동일 행 3카드, 왼쪽 정렬 lucide 아이콘·제목·설명, MD3 기본/비활성 톤, pane 스크롤과 시스템 테마.
- 2026-10-08 (사용자 요청): 진행 방법 카드는 남는 세로 공간을 채우고(창이 작으면 최소 20rem 뒤 스크롤) 큰 아이콘·제목·설명을 가로·세로 가운데 정렬한다. 번호는 표시하지 않고 설명은 단어 단위로 줄바꿈한다. 하단의 자동 진행 설명 문단은 없애고 자동 진행 카드 설명에 합쳤다. 상단 기기 카드(`SelectionDeviceStatus`, 수동·통신·업데이트 선택 화면 공용)는 언락·루팅·VoLTE(셀룰러 IMS 음성 등록) 상태 칩을 표시한다.
- 2026-10-08: 업데이트 카드는 준비 중 사유로 비활성.
