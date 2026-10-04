# Native EFS/NV completion — 2026-10-04

Historical handoff report. The 2026-10-05 integration and follow-up fixes are documented in [full-review-20261005.md](full-review-20261005.md). Hardware validation remains tracked only in [device-test-checklist.md](device-test-checklist.md).

Preserved and integrated on `feat/efs-native` in `../xperia-volte-activator-efs-native`.
Implementation commit: `baef37ef7a9af40b6f11a1f9ce13e7107f9d8efe`.
The original report was committed separately as `ed7c316`. The temporary Orca terminal, managed worktree, added project registration and duplicate `korjwl1/efs-native-rust` branch have now been removed after preservation. No push was performed.

Implemented the Rust COM/HDLC/DIAG session, EFS and independent numeric NV operations, bounded manifests and snapshot loading, contextual structured errors, cancellation/descriptor cleanup, scoped snapshot/verified rollback, offline preset-set conflict validation, and the configured facade/wizard path with per-slot two-pass upload and readback. Design, compatibility evidence, reproduction inputs and caller migration: [efs-native.md](efs-native.md). Contracts and affected view/mock documents are updated in the implementation commit.

## Final affected checks

All commands ran from this checkout. Rust commands used the MSVC environment prefix:

```powershell
cmd /d /s /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri/Cargo.toml efs:: -- --test-threads=1'
cmd /d /s /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri/Cargo.toml --features efs-write efs:: -- --test-threads=1'
cmd /d /s /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo build --manifest-path src-tauri/Cargo.toml --features efs-write'
$env:EFS_STOCK_ROOT='C:/Users/korjw/Downloads/sony-volte_v0.1-beta11_20260821'
cmd /d /s /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri/Cargo.toml efs::tests::original_balance_hashes_match_pinned_manifest_without_changing_assets -- --ignored --exact && cargo build --manifest-path src-tauri/Cargo.toml'
rustfmt --edition 2021 --check src-tauri/src/efs/mod.rs
pnpm.cmd check
pnpm.cmd build
git -c core.safecrlf=false diff --check
git -c core.safecrlf=false diff --cached --check
```

| Check | Result |
| --- | --- |
| Default-gated EFS tests | 15 passed, 1 ignored, 60 filtered; no failures |
| Feature-enabled EFS tests, fake transports only | 15 passed, 1 ignored, 60 filtered; no failures |
| Read-only original stock test | 1 passed, 75 filtered; validates all eight approved balance hashes and actual shared-target conflicts/same-value compatibility |
| Rust builds, feature-enabled then default | Both passed; neither binary executed |
| Native module formatting / staged whitespace | Passed |
| Svelte check | 0 errors, 0 warnings |
| Frontend production build | Passed |

The frontend commands also print the existing tsconfig `baseUrl`/`paths` advisory. Rust emits the Windows import-library creation linker message. Neither caused a failed check.

An earlier full Rust run passed 69 tests with 6 ignored before the final native-only edits/additional test. The final rerun was deliberately limited to affected checks above. No live ignored test was executed; the explicitly selected ignored stock test only reads local files.

C# golden fixtures were regenerated with `C:/Users/korjw/AppData/Local/Microsoft/dotnet/dotnet.exe` (.NET 8), not PATH's .NET 5. They cover request encodings, suppression sequence, upstream status constants and measured NV logical sizes. The stock inventory covers all 16 balance/performance manifests using filename/length/hash facts, without redistributing payloads. Source inputs and fixture reproduction commands are pinned in the design document.

Coordinator-verified original-bundle parity: the coordinator independently executed the built `scripts/efs-golden/bin/Debug/net8.0/Golden.dll` against the actual bundle's `util/EfsTools/EfsTools.dll` and util root. Original DLL SHA-256: `040b94d6e68c2156326ba029668dd71ba32f0ae345ad689078f60759ebfb4704`. All **13 packet fixtures, 45 measured NV sizes and four error constants** matched the checked-in fixture entries, with **zero differences and exit 0**. Equality between the cached-build fixture output and the original shipped DLL is therefore verified for these entries; it is not an outstanding hardware limitation.

The fake tests exercise fragmented/coalesced frames, CRC corruption, partial writes, unsolicited/out-of-order replies, timeout poisoning, cancellation, structured device errors, malformed/short PUT replies, two-pass uploads, EFS/NV readback, and a real mock rollback roundtrip restoring general/item EFS metadata/type and full 128-byte NV, removing newly created targets and closing descriptors. Upstream PUT is opcode **38**; opcode 37 is Deltree. `DirectoryExist` is **6**, and suppression preserves ranges/SetMask parity.

## Execution limits and compatibility decisions

- Cargo `efs-write` defaults off; `REAL_STEPS.efs` remains false. No real COM open, DIAG transition, phone read/write, rollback, USB probing or app launch occurred. The last Rust build used default gates. Other worktrees and original shipped presets were untouched. The handoff task file was read and removed.
- Hardware compatibility remains unverified: explicit COM-to-phone identity, qcser behavior, authentication/suppression support, decimal flags/PUT padding, short NV acceptance, sync durability, disconnect cleanup and carrier/IMS behavior require separately authorized device validation. Defaults preserve 38400 baud and 7000 ms response timeout.
- Empty KT NV entries 6789/6849 are visibly skipped and excluded from written/verified counts. Empty bytes cannot prove a changed value. This intentionally differs from the original empty request plus hidden failure; those entries remain unresolved rather than claimed successful.
- Numeric NV IDs are global. Differing shared contents/mode/type are rejected for the entire selection before DIAG/mutation. Stock SKT1+KT2 conflicts at 71/1920; SKT1+LGU2 at 71/6862. Same values may proceed. Mixed-carrier conflict resolution is outside this module.
- Known NV logical sizes are checked against C# measurements; unknown short values receive an explicit prefix-verification warning. Unspecified tail bytes are not claimed verified. Snapshot/rollback preserve and verify full 128-byte values.
- Snapshots cover planned active targets, not the whole modem filesystem. Restore verifies content and EFS mode/type; captured timestamps are not reapplied, and newly created parent directories remain. No automatic rollback occurs. Sequential slot snapshots must be restored in reverse capture order to unwind shared targets.
- COM configuration has a concrete facade/app-local JSON path, with no selection screen or guessed port. Direct callers must validate their full selected preset set before device operations. Physical phone association remains the caller's responsibility.

## Reviewed application integration — 2026-10-05

The native branch includes committed `fix/full-review` (`9323498`) through merge `9a419e9` and consolidates `feat/efs-wrapper` (`ccd4806`) with both histories retained. Claude's uncommitted work in the main checkout was neither copied nor changed. Driver and verification worktrees remain separate.

Integration merge: `291b7a0`. All three input tips were verified as ancestors before deleting the obsolete `feat/efs-wrapper` branch. Its Git worktree registration is removed. Git could not fully delete the old `../xperia-volte-activator-efs` directory (`Directory not empty`); the subsequent bounded leftover-directory cleanup was rejected by automatic policy review. That unregistered directory remains, and no policy bypass was attempted. The temporary Orca resources described above were successfully removed and their absence verified.

The old `src-tauri/src/efstools` subprocess implementation and `scripts/build-efstools.ps1` are removed. Existing VoLTE property/reboot and IMS communication/final verification stages remain connected to the reviewed wizard engine. `volte_props_set` now uses explicit selected-device ADB with the native EFS execution gate, exclusive operation owner and cancellation token. The EFS facade uses the existing transport boundary, preserving structured native errors. Generation checks release late event listeners and prevent a cancelled/reset run from starting DIAG. Both the frontend and Rust execution gates remain off by default.

Final integrated validation:

- Full default Rust library suite: **186 passed, 7 ignored, 0 failed** (193 tests).
- All-feature EFS suite: **15 passed, 1 ignored, 0 failed**; the separately selected read-only stock manifest test also passed.
- Rust builds with all features and then default gates: both passed; executables were not launched.
- Frontend suite: **53 passed, 0 failed**, including six native integration tests covering facade gates, structured failures/COM arguments, reset races, whole-selection conflict validation, per-slot snapshots/two uploads/single dispatch, and retained VoLTE/IMS stages.
- Svelte check: **0 errors, 0 warnings**. Frontend production build passed.

The original DLL parity and device limitations above still apply. This completes local implementation and branch consolidation; it does not establish real-phone compatibility or deploy the changes into Claude's active checkout.
