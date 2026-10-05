# component: CommunicationPanel

status: implemented

Shared read-only diagnostic card in PlanReview, RunProgress manual prompts and Finish. Props: snapshot/loading/error/slots/calls/showCalls, onRefresh and onCallChange. It performs no IPC; callbacks go through Wizard and the API facade. It renders reason, registration, voice/SMS availability, transport/technology, observation time and firmware/baseband/preset metadata. Optional user call checks distinguish outgoing/incoming/two-way audio from reboot/idle persistence. Unknown technology never becomes an LTE claim; missing SIM never blocks writes. Existing pane/modal scrolling, MD3 tokens, lucide icons and shadcn controls are retained.
