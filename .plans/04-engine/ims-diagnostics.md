# IMS diagnostics follow-up, 2026-10-05

Scope: non-model-specific findings from the latest verification research. No 10-series-only activation/profile changes. User-selected carrier and slot remain authoritative regardless of detected SIM; communication verification remains optional. Device write features and REAL_STEPS stay disabled by default.

Implementation:

- Pure `ims.rs` handles old/new transport log names, current registration, capability, optional technology and conflicting/stale events. No IMS provisioning or network edits.
- Device status returns bounded summaries and explicit query/no-SIM/not-ready/unsupported/registration reasons. Voice and SMS are separate, and cellular transport alone does not assert LTE rather than NR.
- Shared communication domain helpers and panel. Pre-run and post-completion observation reuse the existing facade. No dialling, SMS sending, APN changes or automatic LTE-only forcing.
- Slot-level user call evidence; read-only metadata/preset snapshots. Request generations, selection and manual-watch checks discard late responses, including replies after skipping confirmation.
- Completed diagnostic updates save and archive in one ordered queue job. Save failures cannot archive older data. Legacy journals reopen communication checks; file readback remains independent.

Primary sources checked during research:

- https://android.googlesource.com/platform/frameworks/opt/telephony/+/master/src/java/com/android/internal/telephony/imsphone/ImsPhone.java
- https://android.googlesource.com/platform/packages/services/Telephony/+/refs/heads/main/src/com/android/phone/settings/RadioInfo.java
- https://developer.android.com/reference/android/telephony/ims/RegistrationManager
- https://developer.android.com/reference/android/telephony/ims/stub/ImsRegistrationImplBase
- https://developer.android.com/reference/kotlin/android/telephony/TelephonyDisplayInfo

Limit: only supported dump fields are interpreted; unsupported firmware formats remain unknown. Network readiness and byte verification cannot prove actual call audio, SMS/MMS, roaming, long-term stability or modem/preset compatibility. Those require separate device evidence; no new compatibility guarantees are added.

Offline validation: frontend 93 passed; svelte-check 0 errors/0 warnings; production build passed. Rust 211 passed/8 ignored in both default and all-feature builds; strict all-target/all-feature Clippy passed; diff whitespace validation passed. No physical-device tests or installer rebuild. Dependency links were repaired using the frozen lockfile and this worktree has independent node_modules.
