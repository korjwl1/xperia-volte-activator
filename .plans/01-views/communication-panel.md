# component: CommunicationPanel

status: implemented

Shared read-only diagnostic card in PlanReview, RunProgress manual prompts and Finish. Props: snapshot/loading/error/slots/calls/showCalls, onRefresh and onCallChange. It performs no IPC; callbacks go through Wizard and the API facade. It renders reason, registration, voice/SMS availability, transport/technology, observation time and firmware/baseband/preset metadata. Optional user call checks distinguish outgoing/incoming/two-way audio from reboot/idle persistence. Unknown technology never becomes an LTE claim; missing SIM never blocks writes. Existing pane/modal scrolling, MD3 tokens, lucide icons and shadcn controls are retained.
2026-10-06 조회 전 슬롯 자리표시의 SIM 유형은 unknown이다. 물리 SIM으로 가정하지 않으며 저장 기록 검증에서도 unknown을 허용한다. 비주얼 변경 없음.
