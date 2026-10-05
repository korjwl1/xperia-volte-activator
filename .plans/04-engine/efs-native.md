# Native Rust EFS/NV replacement

Status: integrated with the reviewed backup/root/fastboot/verification engines, 2026-10-05. See [full integration review](full-review-20261005.md). Hardware compatibility is unverified. No real phone operation, ADB DIAG switching, COM opening or USB probing was performed by the EFS tests. Cargo `efs-write` and frontend `REAL_STEPS.efs` remain disabled by default.

`src-tauri/src/efs/` replaces the committed `feat/efs-wrapper` reference at `ccd4806` without importing its unrelated root/firmware runners. The old executable wrapper, stdout cancellation, temporary directory comparison and .NET runtime requirement are absent from this checkout. C# is used only to regenerate developer fixtures. The frontend keeps `api.efsUpload(preset.folder)` / `api.efsVerify(preset.folder)` and the slot order / two passes; snapshot now requires the preset folder too.

## Layers and ownership

| Module | Responsibility |
| --- | --- |
| transport | serialport COM transport, explicit `COM<number>`, 38400 baud, 8N1, no flow control, 50 ms I/O polling |
| hdlc | CRC16/X25, low byte then high byte, escaping CRC and payload, optional leading delimiter, required trailer; streaming fragments/coalesced frames, bounded receive |
| wire | C# compatible little-endian requests, checked primitive reads, EFS/NV status handling and NV logical lengths |
| session | one request at a time, 7000 ms overall exchange deadline (partial TX included), cancellation token, response matching and known descriptor ownership |
| device | hello/query/stat/open/read/write/close/unlink/mkdir/opendir/readdir/closedir/PUT/SyncNoWait and independent numeric NV reads/writes |
| manifest | bounded local enumeration, metadata suffix parsing, ROOT-only numeric NV manifest, approved balance hashes, shared-target conflict validation |
| engine | upload/readback, structured warnings and progress, scoped before-images, validated offline restore plan and verified rollback |
| config/mod | Tauri commands, app-local configuration, global operation ownership including DIAG transition, default-disabled gates |

An operation claims EFS ownership and the shared `device_io::WriteOperation` lock before preparing work. Concurrent EFS/backup/restore/fastboot/Magisk starts return `busy`; a new request cannot clear another request's cancellation. The owner remains held until known descriptors and transport are released. COM creation is exclusive at the Windows handle layer as well. No port is guessed and no adb executable is launched.

Receive matching checks command, subsystem and opcode, plus descriptor/offset for read/write, iterator handle/sequence for readdir, SyncNoWait sequence, NV ID, and log/message mask scope/range where echoed. Unsolicited packets are consumed. Corrupt/truncated/oversized frames fail closed. Timeout, cancellation and transport failure poison the session: no automatic resend of a mutation whose outcome is uncertain. Cleanup can ignore cancellation solely to close known handles, with a 500 ms deadline per handle; failures are attached to the structured error. Loss during open may leave a remotely allocated descriptor whose ID was never received, which cannot be guaranteed closed.

Every native device command, including reads and setup, requires Cargo `efs-write`. `efs_cancel`, configuration, preset resolution/validation and built-in version checking do not touch the device. The wizard independently requires `REAL_STEPS.efs`. This task did not change either default.

## Wire compatibility evidence

`scripts/efs-golden/Program.cs` reflects only request constructors, `GetData`, the HDLC encoder and `ItemsFactory.SizeOfNvItem` in the cached C# build. It never creates a serial port or QcdmManager. Checked-in `fixtures/upstream.json` contains actual C# output for hello, password, SPC, open, PUT, short/empty NV, HDLC and suppression requests, plus measured NV sizes and status constants. Adapted source notices are retained in `src-tauri/src/efs/NOTICE.md` under upstream MIT terms.

Reproduction inputs are pinned by SHA-256: cached `master.zip` = `f947b9e7e011f1d514d3a82d8fe66b135b71a9b220c62ab9ce800cb74ffe3de8`; cached locally compiled net8.0 C# `EfsTools.dll` used by the fixture harness = `99201687f99d462c0a3d3f718709250f9a9c345e9b1081c2c015b83a602d04fd`. The handoff's original official v0.14/util DLL digest is `040b94d6e68c2156326ba029668dd71ba32f0ae345ad689078f60759ebfb4704`; it is a distinct binary, not the net8.0 fixture build. No external archive or executable is bundled with this implementation.

The coordinator independently ran the built golden harness against that actual original bundle DLL and its util root: all 13 packet fixtures, 45 measured NV sizes and four error constants matched the checked-in entries, with zero differences and exit 0. Wire-fixture equality with the shipped DLL is verified for this coverage; device acceptance remains untested.

- `Create=00100` in C# is decimal 100; default permission `0777` is decimal 777. Native does not reinterpret these as octal.
- Password sends eight ASCII `F` bytes; SPC sends six ASCII `0` bytes.
- EFS PUT is opcode **38**, while 37 is Deltree. PUT retains the upstream allocation: u16 data length plus two zeros, flags at 8, low mode bytes at 12, data at 14, path and NUL, then ten allocation zeros. Response mode/status/count are u16 at 4/6/8. Errors and short counts are checked.
- `DirectoryExist=6`, `NoEntry=2`, `FileExist=17`, `InvalidSequence=0x40000002` are distinct. Only the directory-exists code is tolerated by mkdir. Iteration uses the original documented InvalidSequence termination, or an empty name.
- Suppression preserves the upstream log ranges → zero SetMask(3), then message ranges → zero SetMask(4) sequence, including the C# request allocation padding. Authentication/suppression may tolerate explicit matching BAD_CMD and surface a warning. Timeout/denial/malformed responses remain failures; EFS/NV required operations cannot be ignored.
- Unlike upstream's CRC `&&` comparison, both received CRC bytes must match. No assumption is made that one OS read equals one DIAG frame.

Numeric NV response layout is additionally corroborated by [ModemManager's primary libqcdm header](https://github.com/linux-mobile-broadband/ModemManager/blob/main/libqcdm/src/dm-commands.h): command + u16 ID + 128 data bytes + u16 status. Native requires the full 133-byte reply and checks status at offset 131; it never treats the echoed ID as write status. This protocol-layout reference is not imported GPL code.

## Preset and NV policies

The production loader accepts only the eight existing balance folder-name/SHA-256 pairs in `efsPresets.ts`. Renaming another approved carrier/slot's contents is rejected. Hashing is sorted relative path + NUL + exact contents + NUL. Names, case and fullwidth underscores stay unchanged. Paths are resolved from a configured bundle root, with local symlink/reparse/traversal checks. Phone paths use validated printable ASCII; local paths may contain Korean/spaces. Limits: 16 MiB/file, 128 MiB/preset, 10,000 entries, depth 64, 8 KiB/frame. Item PUT is limited to 2048 bytes.

| Stock balance preset | All targets | ROOT numeric NV | Empty NV skipped | Active verification targets |
| --- | ---: | ---: | ---: | ---: |
| SKT1 / SKT2 | 82 / 82 | 14 / 14 | 0 / 0 | 82 / 82 |
| KT1 / KT2 | 116 / 114 | 18 / 18 | 2 / 2 | 114 / 112 |
| LGU1 / LGU2 | 101 / 96 | 15 / 15 | 0 / 0 | 101 / 96 |
| LGU for V 1 / 2 | 67 / 65 | 0 / 0 | 0 / 0 | 67 / 65 |

The inventory covers all 723 balance files (94 ROOT NV) and all 1046 performance files (189 numeric NV filenames, some nested). Performance remains an audited reference and is not a production-approved preset. No original assets are modified or bundled. Checked-in stock fixtures contain only names/lengths/hashes. The read-only opt-in test validates actual original bytes and all eight approved hashes.

Numeric NV is separate from filesystem `/nv/...` items. Only manifest IDs are read/written; there is no full 65,534-ID scan. Original `-v` means **processNvItems**, not verbosity, and original `-n` omits metadata suffixes. Native directly verifies each planned target; file metadata is stored separately for restore rather than mixed into comparison filenames.

Non-empty NV writes preserve the exact short request payload, with no padding/truncation. Read replies remain raw 128 bytes. For supported items with a measured nonzero ItemsFactory logical size, the preset must have that size and verification compares those bytes. For unrecognized logical sizes, verification compares the explicit preset prefix and returns `nvPrefixVerification` if it is shorter than 128 bytes. The unspecified tail is neither invented nor claimed changed. Snapshot/rollback always preserve and verify all 128 bytes, including the unspecified tail.

**Empty NV policy:** KT1/2 contain zero-byte IDs 6789 and 6849 even though upstream logical sizes are one byte. An empty payload cannot establish an expected changed value. Original sends a three-byte command and silently catches failures. Native performs no read or mutation for these entries, returns `emptyNvSkipped` warnings, and excludes them from written/verified counts and progress totals. A report can confirm all active targets while explicitly recording these omissions; it cannot claim the empty entries were written or verified.

**Shared-target policy:** numeric NV IDs are global, not per-SIM. Before DIAG/mutations, the wizard validates the entire selected preset set and rejects differing contents, EFS mode or entry type for any shared target. Identical values are allowed. Stock SKT1+KT2 disagree at NV 71/1920, and SKT1+LGU2 at NV 71/6862. They return `presetConflict` before phone access. This is an explicit mixed-carrier limitation; native does not invent subscription semantics or silently drop NV checks.

## Before-image and restoration

The wizard snapshots every active target before the first pass for each slot. It captures existing EFS contents, mode/type/times and raw NV, records explicit absent EFS targets, and writes a versioned incomplete manifest before reading. Only a fully successful snapshot publishes `complete:true`. Blob names are generated numbers independent of phone filenames. Errors/cancellation leave an unusable incomplete snapshot. Capture and restore share a 128 MiB aggregate limit and a 4 MiB manifest limit. Blobs and manifest use the shared synchronized atomic writer; publishing an oversized completion cannot erase the incomplete marker.

Rollback loads and validates every entry, path, duplicate, length, type/mode and SHA-256 before opening COM. It restores general files with open/write and item files with PUT, restores full numeric NV, removes EFS targets absent before upload, and reads everything back. It verifies restored EFS mode/type too. Times are recorded for review but not reapplied by this first module; parent directories created during upload are retained. This is a scoped target before-image, not an entire modem-filesystem archival backup.

Restore is caller-requested through the gated facade; no automatic rollback occurs after partial failure. Per-slot snapshots are logged with their paths. If restoring several sequentially captured slot snapshots, the caller must use reverse capture order to unwind shared values. On physical disconnect or lost acknowledgement, remote outcomes and cleanup cannot be guaranteed; the original snapshot remains the reviewable input.

## Configuration and caller migration

`api.efsConfigure({ port, presetRoot, snapshotRoot })` stores `efs-native.json` in Tauri `app_local_data_dir`; it does not open COM. `presetRoot` is the absolute original bundle root **containing util/SonyEFS**. `snapshotRoot` is an absolute user-selected destination. `port` must be explicitly supplied for the chosen phone by the caller; no first-port default exists. Settings can also be prepared by placing the same JSON in the app-local data directory. Setting up COM-to-phone identity and qcser drivers remains a hardware/operator responsibility.

```ts
import { api } from "$lib/api";
// Values come from user settings; configuration alone never operates a phone.
await api.efsConfigure({ port: selectedCom, presetRoot: selectedBundleRoot, snapshotRoot: selectedSnapshotRoot });
// Read-only local preparation: required before any DIAG transition or mutation.
const valid = await api.efsValidatePresets(selectedPresetFolders);
```

The existing facade translates `efsPreset(...).folder` into a bounded approved absolute path through `efs_resolve_preset`, and supplies COM from configuration. Direct native callers now pass `port` and absolute `presetDir`; snapshot also passes `dest`. Direct low-level callers must validate their entire selected set before DIAG or mutation, since a single upload command has no knowledge of the other slot selection. Full signatures and error/result fields are in `02-contracts/tauri-commands.md`.

Browser facade calls keep explicit simulation warnings; Tauri failures return structured native errors rather than simulated success. The wizard activates the native sequence only when the separate frontend flag is deliberately changed. The gated plan-review settings panel saves explicit COM and bundle/snapshot paths. The first `efs-input` step checks configuration, preset identity/conflicts and supported real engine combinations before phone access. The VoLTE properties runner checks all four shell exit codes before rebooting, then waits for disconnect/reconnect; final verification also runs in real mode whenever EFS does.

## Offline validation

Golden fixtures can be regenerated with the available .NET 8 executable (PATH's .NET 5 cannot target this harness):

```powershell
& 'C:/Users/korjw/AppData/Local/Microsoft/dotnet/dotnet.exe' run --project scripts/efs-golden/Golden.csproj -- $cachedAssembly $originalUtilRoot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/efs-stock-fixtures.ps1 -BundleRoot $originalBundleRoot
```

Rust tests use fake/replay transports only: strict CRC including one-byte corruption; fragmented/escaped/coalesced/oversized frames; unsolicited/out-of-order replies; short OS and EFS writes; timeouts/poisoning; cancellation/descriptor cleanup; error/short PUT replies; status constants; two passes plus EFS and ROOT NV readback; empty/prefix policies; stock conflicts; corruption rejection and actual restore roundtrip with metadata and newly created file removal.

Exact command results and commit information are recorded in `efs-native-completion.md`. All live tests remain unexecuted. DIAG setup/authentication support, C# decimal flag quirks/PUT padding, short NV acceptance, actual sync durability, qcser behavior, physical COM identity, disconnect recovery and carrier/IMS functionality need separately authorized hardware validation.

2026-10-05 follow-up: DIAG reads the selected device model and uses checked shell statuses. Mark IV XQ-CT/XQ-CQ applies and reads back persist.usb.eng before USB configuration, without SIM queries. Missing Mark II PDC and Mark IV KT/LGU modem procedures are rejected before device changes. PRO-I uses boot. Final IMS polling updates the prompt without automatically closing it. File readback, IMS registration and optional user call confirmation are independent outcomes. See cafe-flow-hardening.md.
