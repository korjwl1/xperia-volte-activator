use super::{
    engine::{self, ImageTask, Journal, Record},
    package,
    policy::{self, Disposition},
    protocol::{Reply, SignatureMode, S1},
    sin,
    transport::FlashTransport,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, VecDeque},
    io::{Cursor, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, bytes) in members {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *bytes).unwrap();
    }
    builder.into_inner().unwrap()
}
fn sample() -> Vec<u8> {
    archive(&[
        ("boot.cms", b"TEST-CMS"),
        ("boot.000", b"first"),
        ("boot.001", b"second"),
    ])
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn task(dir: &std::path::Path) -> ImageTask {
    let bytes = sample();
    let path = dir.join("boot_X-FLASH-ALL-test.sin");
    std::fs::write(&path, &bytes).unwrap();
    ImageTask {
        path,
        relative_path: "boot_X-FLASH-ALL-test.sin".into(),
        file_sha256: sha(&bytes),
        index: sin::inspect(&mut Cursor::new(bytes)).unwrap(),
        target: "boot_b".into(),
    }
}
enum Step {
    Out(Vec<u8>),
    In(Vec<u8>),
    Fail,
}
struct Fake {
    steps: VecDeque<Step>,
    max_write: usize,
    sent: Vec<Vec<u8>>,
    offset: usize,
}
impl Fake {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: steps.into(),
            max_write: usize::MAX,
            sent: Vec::new(),
            offset: 0,
        }
    }
}
impl FlashTransport for Fake {
    fn write(&mut self, data: &[u8], timeout: Duration) -> super::Result<usize> {
        assert!(!timeout.is_zero());
        let Some(Step::Out(expected)) = self.steps.front() else {
            panic!("Unexpected transport write");
        };
        let n = self
            .max_write
            .min(data.len())
            .min(expected.len() - self.offset);
        assert_eq!(&data[..n], &expected[self.offset..self.offset + n]);
        self.offset += n;
        if self.offset == expected.len() {
            self.sent.push(expected.clone());
            self.steps.pop_front();
            self.offset = 0;
        }
        Ok(n)
    }
    fn read_response(&mut self, _: usize, timeout: Duration) -> super::Result<Vec<u8>> {
        assert!(!timeout.is_zero());
        match self.steps.pop_front() {
            Some(Step::In(bytes)) => Ok(bytes),
            Some(Step::Fail) => Err("FLASH_DISCONNECTED|test disconnect".into()),
            _ => panic!("Unexpected response read"),
        }
    }
}
fn out(value: &str) -> Step {
    Step::Out(value.as_bytes().to_vec())
}
fn ack() -> Step {
    Step::In(b"OKAY".to_vec())
}
fn data(n: usize) -> Step {
    Step::In(format!("DATA{n:08x}").into_bytes())
}
fn flag(active: bool) -> Vec<Step> {
    vec![
        out("download:00000001"),
        data(1),
        Step::Out(vec![u8::from(active)]),
        ack(),
        out("Write-TA:2:10100"),
        ack(),
    ]
}
fn script() -> Vec<Step> {
    script_for(SignatureMode::DownloadThenSignature)
}
fn script_for(mode: SignatureMode) -> Vec<Step> {
    let mut steps = flag(true);
    steps.extend([
        out(if matches!(mode, SignatureMode::Legacy) {
            "signature:00000008"
        } else {
            "download:00000008"
        }),
        data(8),
        out("TEST-CMS"),
        ack(),
    ]);
    if matches!(mode, SignatureMode::DownloadThenSignature) {
        steps.extend([out("signature"), ack()]);
    }
    steps.extend([
        out("download:00000005"),
        data(5),
        out("first"),
        ack(),
        out("erase:boot_b"),
        ack(),
        out("flash:boot_b"),
        ack(),
    ]);
    steps.extend([
        out("download:00000006"),
        data(6),
        out("second"),
        ack(),
        out("flash:boot_b"),
        ack(),
    ]);
    steps.extend(flag(false));
    steps.extend([out("Sync"), ack()]);
    steps
}
#[derive(Default)]
struct MemoryJournal {
    records: Vec<Record>,
    fail_at: Option<usize>,
}
impl Journal for MemoryJournal {
    fn store(&mut self, record: &Record) -> super::Result<()> {
        if self.fail_at == Some(self.records.len()) {
            return Err("FLASH_JOURNAL|test write failure".into());
        }
        self.records.push(record.clone());
        Ok(())
    }
}

#[test]
fn sin_supports_multiple_chunks_and_gzip_without_extraction() {
    let bytes = sample();
    let expected = sin::inspect(&mut Cursor::new(&bytes)).unwrap();
    assert_eq!(expected.members.len(), 3);
    assert_eq!(expected.partition, "boot");
    assert_eq!(expected.members[2].sha256, sha(b"second"));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let mut actual = sin::inspect(&mut Cursor::new(encoder.finish().unwrap())).unwrap();
    assert!(actual.compressed);
    actual.compressed = false;
    assert_eq!(actual, expected);
}
#[test]
fn sin_rejects_missing_signature_wrong_target_duplicate_and_chunk_gaps() {
    for bytes in [
        archive(&[("boot.000", b"image")]),
        archive(&[("boot.cms", b"sig"), ("boot.001", b"image")]),
        archive(&[("boot.cms", b"sig"), ("boot.000", b"a"), ("boot.000", b"b")]),
        archive(&[("boot.cms", b"sig"), ("modem.000", b"image")]),
    ] {
        assert!(sin::inspect(&mut Cursor::new(bytes)).is_err());
    }
}
#[test]
fn sin_rejects_checksum_truncation_gzip_crc_and_data_after_end() {
    let mut checksum = sample();
    checksum[0] ^= 1;
    assert!(sin::inspect(&mut Cursor::new(checksum)).is_err());
    let bytes = sample();
    assert!(sin::inspect(&mut Cursor::new(&bytes[..600])).is_err());
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let mut gzip = encoder.finish().unwrap();
    let len = gzip.len();
    gzip[len - 8] ^= 1;
    assert!(sin::inspect(&mut Cursor::new(gzip)).is_err());
    let mut trailing = bytes;
    trailing.extend(b"hidden");
    assert!(sin::inspect(&mut Cursor::new(trailing)).is_err());
}

#[test]
fn sin_rejects_links_and_oversized_advertised_members_before_allocating() {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o644);
    header.set_path("boot.cms").unwrap();
    header.set_link_name("outside").unwrap();
    header.set_cksum();
    let mut link = header.as_bytes().to_vec();
    link.extend([0u8; 1024]);
    assert!(sin::inspect(&mut Cursor::new(link)).is_err());
    let mut header = tar::Header::new_gnu();
    header.set_size(16 * 1024 * 1024 + 1);
    header.set_mode(0o644);
    header.set_path("boot.cms").unwrap();
    header.set_cksum();
    assert!(sin::inspect(&mut Cursor::new(header.as_bytes()))
        .unwrap_err()
        .starts_with("FLASH_SIN_SIZE|"));
}
#[test]
fn policy_preserves_state_and_blocks_disguised_or_unknown_targets() {
    for target in [
        "modem", "dsp", "userdata", "metadata", "persist", "misc", "frp",
    ] {
        assert_eq!(
            policy::decide(
                &format!("{target}_X-FLASH-ALL-test.sin"),
                Some(target),
                &BTreeSet::new()
            )
            .unwrap()
            .disposition,
            Disposition::Preserve
        );
    }
    assert_eq!(
        policy::decide("boot/test.ta", None, &BTreeSet::new())
            .unwrap()
            .disposition,
        Disposition::Preserve
    );
    assert!(policy::decide("boot_X-FLASH-ALL-test.sin", Some("modem"), &BTreeSet::new()).is_err());
    assert_eq!(
        policy::decide("mystery.sin", Some("mystery"), &BTreeSet::new())
            .unwrap()
            .disposition,
        Disposition::Block
    );
    assert_eq!(
        policy::decide(
            "boot_X-FLASH-ALL-test.sin",
            Some("boot"),
            &BTreeSet::from(["boot_x-flash-all-test.sin".into()])
        )
        .unwrap()
        .disposition,
        Disposition::Preserve
    );
}
#[test]
fn windows_aliases_and_path_escapes_are_rejected_on_every_platform() {
    for path in [
        "../boot.sin",
        "/boot.sin",
        "a//b.sin",
        "a\\boot.sin",
        "boot.sin:stream",
        "NUL.sin",
        "COM1",
        "boot.sin.",
        "a/./b.sin",
    ] {
        assert!(policy::safe_relative(path).is_err(), "{path}");
    }
}
#[test]
fn xml_preservation_list_is_structural_and_fingerprint_must_be_unique() {
    let (fingerprint,files)=package::update_metadata("<UPDATE><FINGERPRINT>Sony/target</FINGERPRINT><NOERASE><FILE>metadata.sin</FILE><FILE>userdata.sin</FILE></NOERASE></UPDATE>").unwrap();
    assert_eq!(fingerprint, "Sony/target");
    assert_eq!(files.len(), 2);
    for xml in [
        "<UPDATE><FINGERPRINT>x</FINGERPRINT><FINGERPRINT>x</FINGERPRINT></UPDATE>",
        "<UPDATE><FINGERPRINT>x</FINGERPRINT><NOERASE/></UPDATE>",
        "<UPDATE><FINGERPRINT>x</FINGERPRINT><NOERASE file='metadata.sin'/></UPDATE>",
        "<UPDATE><FINGERPRINT>x</FINGERPRINT><NOERASE>../metadata.sin</NOERASE></UPDATE>",
    ] {
        assert!(package::update_metadata(xml).is_err());
    }
}
#[test]
fn package_report_is_deterministic_preserves_inputs_and_cannot_enable_writes() {
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    std::fs::write(
        dir.path().join("update.xml"),
        "<UPDATE><FINGERPRINT>Sony/target</FINGERPRINT><NOERASE>metadata.sin</NOERASE></UPDATE>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("metadata.sin"),
        archive(&[("metadata.cms", b"sig"), ("metadata.000", b"private")]),
    )
    .unwrap();
    let first = package::inspect(dir.path(), "Sony/target").unwrap();
    let second = package::inspect(dir.path(), "Sony/target").unwrap();
    assert_eq!(first.manifest_sha256, second.manifest_sha256);
    assert!(!first.write_ready);
    assert_eq!(
        first
            .files
            .iter()
            .find(|f| f.relative_path == "metadata.sin")
            .unwrap()
            .decision
            .disposition,
        Disposition::Preserve
    );
    assert_eq!(std::fs::read(image.path).unwrap(), sample());
    assert!(package::inspect(dir.path(), "Sony/wrong").is_err());
}
#[test]
fn package_blocks_case_collisions_and_invalid_xml() {
    let dir = tempfile::tempdir().unwrap();
    task(dir.path());
    std::fs::write(
        dir.path().join("update.xml"),
        "<UPDATE><FINGERPRINT>x</FINGERPRINT></UPDATE>",
    )
    .unwrap();
    std::fs::write(dir.path().join("extra.xml"), "not XML").unwrap();
    assert!(package::inspect(dir.path(), "x").is_err());
    // Case collisions occur on Linux and case-sensitive Windows directories; registry is platform-neutral.
    let mut names = BTreeSet::new();
    assert!(names.insert(policy::safe_relative("boot.sin").unwrap()));
    assert!(!names.insert(policy::safe_relative("BOOT.SIN").unwrap()));
}
#[test]
fn modern_transfer_erases_once_commits_every_chunk_and_handles_partial_writes() {
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    let mut transport = Fake::new(script());
    transport.max_write = 2;
    let mut s1 = S1::new(transport, 1024, SignatureMode::DownloadThenSignature).unwrap();
    let mut journal = MemoryJournal::default();
    engine::run(&mut s1, &[image], &mut journal, &AtomicBool::new(false)).unwrap();
    let transport = s1.into_transport();
    assert!(transport.steps.is_empty());
    assert_eq!(
        transport
            .sent
            .iter()
            .filter(|b| b.as_slice() == b"erase:boot_b")
            .count(),
        1
    );
    assert_eq!(
        transport
            .sent
            .iter()
            .filter(|b| b.as_slice() == b"flash:boot_b")
            .count(),
        2
    );
    assert_eq!(
        journal.records.last().unwrap().state,
        "written-not-boot-verified"
    );
}
#[test]
fn rust_image_transfer_matches_pinned_c_reference_for_both_signature_modes() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream.json")).unwrap();
    assert_eq!(fixture["upstreamCommit"], super::UPSTREAM_COMMIT);
    for (name, mode) in [
        ("modern", SignatureMode::DownloadThenSignature),
        ("legacy", SignatureMode::Legacy),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let image = task(dir.path());
        let mut s1 = S1::new(Fake::new(script_for(mode)), 1024, mode).unwrap();
        engine::run(
            &mut s1,
            &[image],
            &mut MemoryJournal::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let sent = s1.into_transport().sent;
        let expected: Vec<Vec<u8>> = fixture[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| hex::decode(v.as_str().unwrap()).unwrap())
            .collect();
        // Slot selection is performed before the Rust runner; C queries has-slot inside process_sins.
        assert_eq!(
            expected
                .iter()
                .filter(|v| v.starts_with(b"getvar:"))
                .count(),
            1
        );
        let normalized: Vec<_> = expected
            .into_iter()
            .filter(|v| !v.starts_with(b"getvar:"))
            .collect();
        // Separate session-enter/exit and Sync behavior is covered by the complete script tests.
        assert_eq!(&sent[3..sent.len() - 4], normalized.as_slice(), "{name}");
    }
}
#[test]
fn legacy_signature_is_distinct_and_failure_does_not_trigger_automatic_fallback() {
    let mut s1 = S1::new(
        Fake::new(vec![out("signature:00000003"), data(3), out("sig"), ack()]),
        1024,
        SignatureMode::Legacy,
    )
    .unwrap();
    s1.payload(3, &mut &b"sig"[..], &sha(b"sig"), true).unwrap();
    assert!(s1.into_transport().steps.is_empty());
    let mut s1 = S1::new(
        Fake::new(vec![
            out("signature:00000003"),
            Step::In(b"FAILprivate identifier".to_vec()),
        ]),
        1024,
        SignatureMode::Legacy,
    )
    .unwrap();
    let err = s1
        .payload(3, &mut &b"sig"[..], &sha(b"sig"), true)
        .unwrap_err();
    assert!(!err.contains("private"));
    assert!(s1.into_transport().steps.is_empty());
}
#[test]
fn data_mismatch_and_malformed_frames_stop_before_payload() {
    for reply in [
        b"DATA00000004".to_vec(),
        b"DATA0000000g".to_vec(),
        b"DATA00000003x".to_vec(),
        b"OK".to_vec(),
        b"OTHER".to_vec(),
    ] {
        let mut s1 = S1::new(
            Fake::new(vec![out("download:00000003"), Step::In(reply)]),
            1024,
            SignatureMode::DownloadThenSignature,
        )
        .unwrap();
        assert!(s1
            .payload(3, &mut &b"abc"[..], &sha(b"abc"), false)
            .is_err());
        assert_eq!(s1.into_transport().sent.len(), 1);
    }
}
#[test]
fn protocol_handles_info_and_only_the_known_data_nul_quirk() {
    let mut s1 = S1::new(
        Fake::new(vec![
            out("getvar:test"),
            Step::In(b"INFOworking".to_vec()),
            Step::In(b"DATA00000001\0".to_vec()),
        ]),
        1024,
        SignatureMode::DownloadThenSignature,
    )
    .unwrap();
    assert_eq!(s1.command("getvar:test").unwrap(), Reply::Data(1));
}

#[test]
fn info_flood_and_zero_progress_are_bounded() {
    let mut steps = vec![out("getvar:test")];
    steps.extend((0..4096).map(|_| Step::In(b"INFOworking".to_vec())));
    let mut s1 = S1::new(Fake::new(steps), 1024, SignatureMode::DownloadThenSignature).unwrap();
    assert!(s1
        .command("getvar:test")
        .unwrap_err()
        .starts_with("FLASH_INFO_LIMIT|"));
    assert!(s1.into_transport().steps.is_empty());
    let mut fake = Fake::new(vec![out("getvar:test")]);
    fake.max_write = 0;
    let mut s1 = S1::new(fake, 1024, SignatureMode::DownloadThenSignature).unwrap();
    assert!(s1
        .command("getvar:test")
        .unwrap_err()
        .starts_with("FLASH_SHORT_WRITE|"));
}
#[test]
fn plan_limits_and_mutation_fail_before_session_io() {
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    let mut journal = MemoryJournal::default();
    let mut s1 = S1::new(Fake::new(vec![]), 4, SignatureMode::DownloadThenSignature).unwrap();
    assert!(engine::run(
        &mut s1,
        std::slice::from_ref(&image),
        &mut journal,
        &AtomicBool::new(false)
    )
    .is_err());
    assert!(journal.records.is_empty());
    std::fs::write(&image.path, b"changed").unwrap();
    let mut s1 = S1::new(
        Fake::new(vec![]),
        1024,
        SignatureMode::DownloadThenSignature,
    )
    .unwrap();
    assert!(engine::run(&mut s1, &[image], &mut journal, &AtomicBool::new(false)).is_err());
    assert!(s1.into_transport().sent.is_empty());
}
#[test]
fn journal_intent_failure_stops_before_transport_and_ack_failure_keeps_unknown_intent() {
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    for fail_at in [0, 1] {
        let mut journal = MemoryJournal {
            records: vec![],
            fail_at: Some(fail_at),
        };
        let mut s1 = S1::new(
            Fake::new(if fail_at == 0 { vec![] } else { flag(true) }),
            1024,
            SignatureMode::DownloadThenSignature,
        )
        .unwrap();
        assert!(engine::run(
            &mut s1,
            std::slice::from_ref(&image),
            &mut journal,
            &AtomicBool::new(false)
        )
        .is_err());
        let transport = s1.into_transport();
        assert!(transport.steps.is_empty());
        if fail_at == 0 {
            assert!(transport.sent.is_empty());
        } else {
            assert_eq!(journal.records[0].state, "intent");
            assert_eq!(journal.records.len(), 1);
        }
    }
}
#[test]
fn disconnect_keeps_pending_intent_and_never_reboots_or_retries() {
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    let mut steps = flag(true);
    steps.extend([out("download:00000008"), Step::Fail]);
    let mut s1 = S1::new(Fake::new(steps), 1024, SignatureMode::DownloadThenSignature).unwrap();
    let mut journal = MemoryJournal::default();
    assert!(engine::run(&mut s1, &[image], &mut journal, &AtomicBool::new(false)).is_err());
    assert_eq!(journal.records[2].operation, "signature");
    assert_eq!(journal.records[2].state, "intent");
    assert!(s1.into_transport().steps.is_empty());
}
#[test]
fn cancellation_before_start_performs_no_io() {
    let mut s1 = S1::new(
        Fake::new(vec![]),
        1024,
        SignatureMode::DownloadThenSignature,
    )
    .unwrap();
    let mut journal = MemoryJournal::default();
    assert!(
        engine::run(&mut s1, &[], &mut journal, &AtomicBool::new(true))
            .unwrap_err()
            .starts_with("FLASH_CANCELLED|")
    );
    assert!(journal.records.is_empty());
}
#[test]
fn cancellation_after_flash_ack_does_not_download_next_chunk_or_sync() {
    struct CancelJournal<'a> {
        records: Vec<Record>,
        cancel: &'a AtomicBool,
    }
    impl Journal for CancelJournal<'_> {
        fn store(&mut self, r: &Record) -> super::Result<()> {
            self.records.push(r.clone());
            if r.operation == "flash" && r.state == "ack" {
                self.cancel.store(true, Ordering::Release);
            }
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let image = task(dir.path());
    let steps: Vec<_> = script().into_iter().take(20).collect();
    let cancel = AtomicBool::new(false);
    let mut journal = CancelJournal {
        records: vec![],
        cancel: &cancel,
    };
    let mut s1 = S1::new(Fake::new(steps), 1024, SignatureMode::DownloadThenSignature).unwrap();
    assert!(engine::run(&mut s1, &[image], &mut journal, &cancel)
        .unwrap_err()
        .starts_with("FLASH_CANCELLED|"));
    assert!(s1.into_transport().steps.is_empty());
    assert_eq!(journal.records.last().unwrap().state, "cancelled-partial");
}
#[test]
fn file_journal_is_durable_and_refuses_overwriting_existing_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.json");
    let mut journal = engine::FileJournal::create(&path).unwrap();
    assert!(engine::FileJournal::create(&path).is_err());
    journal
        .store(&Record {
            sequence: 1,
            operation: "flash".into(),
            state: "intent".into(),
            target: Some("boot_b".into()),
            member: Some(1),
        })
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(value["records"][0]["state"], "intent");
}
