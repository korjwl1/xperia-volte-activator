# component: CommunicationPanel

status: implemented

Shared read-only diagnostic card in PlanReview, RunProgress manual prompts and Finish. Props: snapshot/loading/error/slots/calls/showCalls, onRefresh and onCallChange. It performs no IPC; callbacks go through Wizard and the API facade. It renders reason, registration, voice/SMS availability, transport/technology, observation time and firmware/baseband/preset metadata. Optional user call checks distinguish outgoing/incoming/two-way audio from reboot/idle persistence. Unknown technology never becomes an LTE claim; missing SIM never blocks writes. Existing pane/modal scrolling, MD3 tokens, lucide icons and shadcn controls are retained.
2026-10-06 조회 전 슬롯 자리표시의 SIM 유형은 unknown이다. 물리 SIM으로 가정하지 않으며 저장 기록 검증에서도 unknown을 허용한다. 비주얼 변경 없음.
2026-10-07 (사용자 지시): 작업 옵션 화면에서는 이 패널을 쓰지 않는다. 남은 사용처(패치 후 확인, 완료 화면)의 SIM 줄은 "VoLTE 활성/비활성"만 보이고, IMS 상세(등록·음성·SMS·기술)는 표시하지 않는다.
