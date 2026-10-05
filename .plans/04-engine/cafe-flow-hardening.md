# Cafe follow-up, 2026-10-05

User decision: write the selected carrier/slot regardless of SIM presence or detected carrier. Detection never overrides the selection. Missing network service is a communication verification outcome, not an EFS write failure.

Changes:

- Conditional OMD guidance: SKT pre-arrange required registration/SIM movements; KT LTE can retain default OMD; coordinate KT 5G code/APN with patch completion; LGU normally needs no registration.
- DIAG model read, checked shell statuses, Mark IV persist.usb.eng/readback and cancellation checks. No SIM queries.
- PRO-I boot mapping. Pre-device restrictions for unimplemented Mark II PDC and Mark IV KT/LGU modem prerequisite verification. No modem mixing/relock automation added.
- Final prompt observes IMS without automatic advancement; optional call confirmation and immediate finish-without-network path. Pre-unroot communication can also be skipped (RunStep.communicationSkipped), without weakening the separate relock gate. File readback is independent.
- IMS capability alone cannot prove current registration. Require registered state and known transport; otherwise unknown/off.
- Persist independent registration/call results. Validate optional booleans, reopen legacy final verification and invalidate communication results on SIM changes while preserving selection and file verification.

Evidence:

- https://cafe.naver.com/x1smart/617140 — OMD guidance and Elenna comment 63820837.
- https://cafe.naver.com/x1smart/607152 — early KT 5G registration can force 3G.
- https://cafe.naver.com/x1smart/605919 — Hanabi Mark IV DIAG exception.
- https://cafe.naver.com/x1smart/613331 and https://cafe.naver.com/x1smart/608340 — model prerequisites and mixed-modem relock failure.
- https://cafe.naver.com/x1smart/616394 — SIM profile reload and fixed patch distinction.
- https://developer.android.com/reference/android/telephony/ims/RegistrationManager — states 0/1/2 and transport type are separate.

Offline validation, including integration with main at e3102a5:

- Frontend: 83 tests passed; svelte-check reported 0 errors and 0 warnings; Vite production build passed.
- Rust: 206 tests passed and 8 intentionally ignored in both default and all-feature builds.
- Strict Clippy passed for all targets and features; git diff --check passed.
- No physical-device tests were run. Cargo writes and REAL_STEPS remain disabled by default. The packaged installer was not rebuilt.
