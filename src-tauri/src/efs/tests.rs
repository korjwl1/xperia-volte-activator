use super::*;
use manifest::{Plan, Target};
use session::Session;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[test]
fn upstream_golden_bytes_and_constants() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream.json")).unwrap();
    let f = &fixture["fixtures"];
    for (name, bytes) in [
        ("hello", wire::hello()),
        (
            "open",
            wire::open("/nv/test", wire::WRITE_CREATE, 777).unwrap(),
        ),
        ("put", wire::put("/nv/test", 777, &[0x7d, 0x7e, 1]).unwrap()),
        ("nvRead", wire::nv_read(562)),
        ("nvWrite", wire::nv_write(562, &[1]).unwrap()),
        ("nvEmpty", wire::nv_write(6789, &[]).unwrap()),
        ("hdlc", hdlc::encode(&[0x4b, 0x13, 0x7d, 0x7e, 0], false)),
        ("logRanges", wire::log_ranges()),
        ("logMask", wire::log_mask(1, 0, 4099).unwrap()),
        ("messageRanges", vec![0x7d, 1]),
        ("messageMask", wire::message_mask(0, 3).unwrap()),
    ] {
        assert_eq!(hex::encode(bytes), f[name].as_str().unwrap(), "{name}");
    }
    assert_eq!(fixture["errors"]["DirectoryExist"], wire::DIRECTORY_EXISTS);
    assert_eq!(fixture["errors"]["NoEntry"], 2);
    assert_eq!(fixture["errors"]["InvalidSequence"], 0x40000002u32);
    assert_eq!(f["password"], hex::encode([0x46; 9]));
}
#[test]
fn fragmented_coalesced_escaped_frames_and_strict_crc() {
    let data = [0x26, 0, 0, 0x7d, 0x7e, 0];
    let frame = hdlc::encode(&data, true);
    for split in 0..=frame.len() {
        let mut decoder = hdlc::Decoder::default();
        let mut out = decoder.feed(&frame[..split]);
        out.extend(decoder.feed(&frame[split..]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_ref().unwrap(), &data);
    }
    let both = [frame.clone(), hdlc::encode(&[1, 2, 3], false)].concat();
    assert_eq!(hdlc::Decoder::default().feed(&both).len(), 2);
    // One wrong CRC byte was accepted by upstream's && bug; both bytes are strict now.
    for byte in [3, 4] {
        let mut frame = hdlc::encode(&[1, 2, 3], false);
        frame[byte] ^= 1;
        assert_eq!(
            hdlc::Decoder::default().feed(&frame)[0]
                .as_ref()
                .unwrap_err()
                .code,
            "crc"
        );
    }
    let mut decoder = hdlc::Decoder::default();
    let mut long = vec![1; hdlc::MAX_PAYLOAD + 3];
    long.push(0x7e);
    long.extend(hdlc::encode(&[1], false));
    let out = decoder.feed(&long);
    assert_eq!(out[0].as_ref().unwrap_err().code, "oversized");
    assert_eq!(out[1].as_ref().unwrap(), &[1]);
    assert!(hdlc::Decoder::default().feed(&[1, 0x7d, 0x7e])[0].is_err());
}

#[derive(Default)]
struct State {
    files: BTreeMap<String, Vec<u8>>,
    metadata: BTreeMap<String, (u32, u32)>,
    nv: BTreeMap<u16, Vec<u8>>,
    handles: BTreeMap<u32, String>,
    requests: Vec<Vec<u8>>,
    next_fd: u32,
    write_limit: usize,
    cancel_after_write: Option<Arc<AtomicBool>>,
    /// 값이 쓰인 적 없는 NV를 실기기처럼 NV_NOTACTIVE(5)로 답한다
    nv_inactive_missing: bool,
}
struct Fake {
    state: Arc<Mutex<State>>,
    decoder: hdlc::Decoder,
    rx: VecDeque<u8>,
    fragment: usize,
    short_tx: usize,
    timeout: bool,
}
impl Fake {
    fn new(state: Arc<Mutex<State>>) -> Self {
        Self {
            state,
            decoder: hdlc::Decoder::default(),
            rx: VecDeque::new(),
            fragment: 3,
            short_tx: 2,
            timeout: false,
        }
    }
    fn respond(&mut self, req: Vec<u8>) {
        let mut st = self.state.lock().unwrap();
        st.requests.push(req.clone());
        let mut r = if req[0] == 0x4b {
            wire::efs(wire::u16_at(&req, 2).unwrap())
        } else {
            vec![req[0]]
        };
        let p = |at: usize| {
            String::from_utf8(req[at..].iter().take_while(|b| **b != 0).copied().collect()).unwrap()
        };
        match req[0] {
            0x46 | 0x41 => r.push(1),
            0x73 if req[4] == 1 => {
                r = vec![0; 18];
                r[0] = 0x73;
                r[4] = 1;
            }
            0x73 => {
                r = vec![0; 21];
                r[0] = 0x73;
                r[4] = 3;
                r[12] = req[8];
                r[16] = 1;
            }
            0x7d if req[1] == 1 => {
                r = vec![0; 12];
                r[0] = 0x7d;
                r[1] = 1;
                r[10] = 3;
            }
            0x7d => {
                r = req.clone();
                r[6] = 1;
            }
            0x26 | 0x27 => {
                let id = wire::u16_at(&req, 1).unwrap();
                if req[0] == 0x27 {
                    let value = st.nv.entry(id).or_insert(vec![0; 128]);
                    value[..req.len() - 3].copy_from_slice(&req[3..]);
                }
                r.extend(id.to_le_bytes());
                r.extend(st.nv.get(&id).cloned().unwrap_or(vec![0; 128]));
                let inactive = st.nv_inactive_missing && !st.nv.contains_key(&id);
                r.extend(if inactive { 5u16 } else { 0 }.to_le_bytes());
            }
            0x4b => match req[2] {
                0 => r = req.clone(),
                1 => r = wire::words(1, &[255, 1024, 8, 16777216, 1000, 8]),
                15 => {
                    let path = p(4);
                    let data = st.files.get(&path);
                    r = wire::words(
                        15,
                        &[
                            if data.is_some() { 0 } else { 2 },
                            777,
                            data.map_or(0, |d| d.len()) as u32,
                            1,
                            10,
                            20,
                            30,
                        ],
                    );
                }
                2 => {
                    let path = p(12);
                    if wire::u32_at(&req, 4).unwrap() != 0 {
                        st.files.insert(path.clone(), vec![]);
                        st.metadata
                            .insert(path.clone(), (wire::u32_at(&req, 8).unwrap(), 0));
                    }
                    st.next_fd += 1;
                    let fd = st.next_fd;
                    st.handles.insert(fd, path);
                    r = wire::words(2, &[fd, 0]);
                }
                3 => {
                    st.handles.remove(&wire::u32_at(&req, 4).unwrap());
                    r.extend(0u32.to_le_bytes());
                }
                4 => {
                    let fd = wire::u32_at(&req, 4).unwrap();
                    let offset = wire::u32_at(&req, 12).unwrap() as usize;
                    let count = wire::u32_at(&req, 8).unwrap() as usize;
                    let data = &st.files[&st.handles[&fd]];
                    let n = count.min(data.len() - offset).min(7);
                    r = wire::words(4, &[fd, offset as u32, n as u32, 0]);
                    r.extend(&data[offset..offset + n]);
                }
                5 => {
                    let fd = wire::u32_at(&req, 4).unwrap();
                    let offset = wire::u32_at(&req, 8).unwrap() as usize;
                    let path = st.handles[&fd].clone();
                    let n = (req.len() - 12).min(st.write_limit.max(1));
                    let data = st.files.get_mut(&path).unwrap();
                    data.resize(offset + n, 0);
                    data[offset..].copy_from_slice(&req[12..12 + n]);
                    r = wire::words(5, &[fd, offset as u32, n as u32, 0]);
                    if let Some(c) = &st.cancel_after_write {
                        c.store(true, Ordering::Release);
                    }
                }
                8 => {
                    let status = if st.files.remove(&p(4)).is_some() {
                        0
                    } else {
                        2
                    };
                    r.extend((status as u32).to_le_bytes());
                }
                9 => r.extend(wire::DIRECTORY_EXISTS.to_le_bytes()),
                48 => {
                    r.extend(req[4..6].iter());
                    r.extend(1u32.to_le_bytes());
                    r.extend(0u32.to_le_bytes());
                }
                11 => {
                    st.next_fd += 1;
                    let fd = st.next_fd;
                    st.handles.insert(fd, p(4));
                    r = wire::words(11, &[fd, 0]);
                }
                12 => {
                    let fd = wire::u32_at(&req, 4).unwrap();
                    let seq = wire::u32_at(&req, 8).unwrap();
                    let parent = st.handles[&fd].clone();
                    let entries: Vec<_> = st
                        .files
                        .iter()
                        .filter(|(path, _)| path.rsplit_once('/').unwrap().0 == parent)
                        .collect();
                    r = wire::words(12, &[fd, seq, 0]);
                    if let Some((path, data)) = entries.get(seq as usize - 1) {
                        let (mode, ty) = st.metadata.get(*path).copied().unwrap_or((777, 0));
                        r.extend(
                            wire::words(0, &[ty, mode, data.len() as u32, 10, 20, 30])[4..].iter(),
                        );
                        r.extend(path.rsplit_once('/').unwrap().1.as_bytes());
                        r.push(0);
                    } else {
                        r.resize(41, 0);
                    }
                }
                13 => {
                    st.handles.remove(&wire::u32_at(&req, 4).unwrap());
                    r.extend(0u32.to_le_bytes());
                }
                38 => {
                    let n = wire::u32_at(&req, 4).unwrap() as usize;
                    let path = p(14 + n);
                    let mode = wire::u16_at(&req, 12).unwrap();
                    st.files.insert(path.clone(), req[14..14 + n].to_vec());
                    st.metadata.insert(path, (mode as u32, 15));
                    r.extend(mode.to_le_bytes());
                    r.extend(0u16.to_le_bytes());
                    r.extend((n as u16).to_le_bytes());
                }
                _ => panic!("Unimplemented fake opcode {}", req[2]),
            },
            _ => panic!("Unknown request"),
        }
        self.rx.extend(hdlc::encode(&[0x10, 1, 2], false)); // unsolicited logs every exchange
        self.rx.extend(hdlc::encode(&r, false));
    }
}
impl Read for Fake {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if self.timeout || self.rx.is_empty() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let n = b.len().min(self.rx.len()).min(self.fragment);
        for byte in &mut b[..n] {
            *byte = self.rx.pop_front().unwrap();
        }
        Ok(n)
    }
}
impl Write for Fake {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let n = b.len().min(self.short_tx);
        for req in self.decoder.feed(&b[..n]) {
            self.respond(req.unwrap());
        }
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn setup() -> (Session<Fake>, Arc<Mutex<State>>, Arc<AtomicBool>) {
    let st = Arc::new(Mutex::new(State {
        write_limit: 3,
        ..Default::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));
    (
        Session::new(
            Fake::new(st.clone()),
            cancel.clone(),
            Duration::from_millis(100),
        ),
        st,
        cancel,
    )
}
fn plan() -> Plan {
    Plan {
        entries: vec![
            manifest::Entry {
                name: "nv/test".into(),
                target: Target::Efs {
                    path: "/nv/test".into(),
                    mode: 777,
                    entry_type: 0,
                },
                data: (0..35).collect(),
            },
            manifest::Entry {
                name: "NvItem__00000562".into(),
                target: Target::Nv { id: 562 },
                data: vec![1],
            },
            manifest::Entry {
                name: "NvItem__00006789".into(),
                target: Target::Nv { id: 6789 },
                data: vec![],
            },
        ],
        sha256: "offline".into(),
    }
}
#[test]
fn upload_twice_then_readback_covers_root_nv_and_short_io() {
    let (mut s, st, _) = setup();
    s.initialize().unwrap();
    s.query().unwrap();
    let plan = plan();
    let mut progress = vec![];
    for _ in 0..2 {
        let r = engine::upload(&mut s, &plan, &mut |p| progress.push(p)).unwrap();
        assert_eq!(r.files_seen, 2);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.warnings[0].code, "emptyNvSkipped");
    }
    let r = engine::verify(&mut s, &plan, &mut |_| {}).unwrap();
    assert!(r.ok);
    assert_eq!((r.matched, r.files, r.planned, r.skipped), (2, 2, 3, 1));
    let state = st.lock().unwrap();
    assert!(state.handles.is_empty());
    assert_eq!(state.files["/nv/test"], plan.entries[0].data);
    assert!(!state
        .requests
        .iter()
        .any(|r| matches!(r[0], 0x26 | 0x27) && wire::u16_at(r, 1).unwrap() == 6789));
    assert_eq!(state.requests.iter().filter(|r| r[0] == 0x27).count(), 2);
    assert!(progress.iter().all(|p| p.total == 2));
}
#[test]
fn cancel_during_write_closes_descriptor_and_stops_following_nv() {
    let (mut s, st, cancel) = setup();
    st.lock().unwrap().cancel_after_write = Some(cancel);
    let err = engine::upload(&mut s, &plan(), &mut |_| {}).unwrap_err();
    assert_eq!(err.code, "cancelled");
    s.cleanup();
    let st = st.lock().unwrap();
    assert!(st.handles.is_empty());
    assert!(!st.requests.iter().any(|r| r[0] == 0x27));
}
#[test]
fn timeout_poisoned_session_is_not_retried() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut io = Fake::new(state);
    io.timeout = true;
    let mut s = Session::new(
        io,
        Arc::new(AtomicBool::new(false)),
        Duration::from_millis(5),
    );
    assert_eq!(s.query().unwrap_err().code, "timeout");
    assert_eq!(s.query().unwrap_err().code, "poisoned");
}
#[test]
fn malformed_and_nv_failure_status_never_success() {
    for n in 0..133 {
        assert!(wire::nv_response(&vec![0; n], "NV").is_err());
    }
    let mut r = vec![0; 133];
    r[131] = 5;
    assert_eq!(wire::nv_response(&r, "NV").unwrap_err().status, Some(5));
    assert!(wire::put("/x", 777, &vec![0; 2049]).is_err());
}

#[test]
fn nv_lengths_follow_measured_known_sizes_unknown_prefix_never_zero_pad() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/upstream.json")).unwrap();
    for (id, size) in fixture["sizes"].as_object().unwrap() {
        assert_eq!(
            wire::nv_logical_size(id.parse().unwrap()),
            size.as_u64().unwrap() as usize
        );
    }
    let raw: Vec<u8> = (0..128).collect();
    assert_eq!(wire::normalize_nv(562, &raw, 1).unwrap(), vec![0]);
    assert_eq!(wire::normalize_nv(71, &raw, 13).unwrap(), raw[..13]);
    assert!(wire::normalize_nv(562, &raw, 2).is_err());
    assert!(wire::normalize_nv(6789, &raw, 0).is_err());
    assert!(wire::normalize_nv(71, &raw[..13], 13).is_err());
    let mut plan = plan();
    plan.entries.push(manifest::Entry {
        name: "NvItem__00000071".into(),
        target: Target::Nv { id: 71 },
        data: raw[..13].to_vec(),
    });
    assert!(engine::warnings(&plan)
        .iter()
        .any(|w| w.code == "nvPrefixVerification"));
}
#[test]
fn manifest_metadata_duplicates_and_root_nv_policy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nv")).unwrap();
    std::fs::write(dir.path().join("nv/item__1FF_F"), [1, 2]).unwrap();
    std::fs::write(dir.path().join("NvItem__00006789"), []).unwrap();
    let plan = Plan::load(dir.path()).unwrap();
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.active().count(), 1);
    assert!(
        matches!(&plan.entries[1].target,Target::Efs{path,mode:511,entry_type:15} if path=="/nv/item")
    );
    std::fs::write(dir.path().join("nv/item__309_0"), [1, 2]).unwrap();
    assert_eq!(Plan::load(dir.path()).unwrap_err().code, "duplicateTarget");
}
#[test]
fn snapshot_retains_metadata_and_full_nv_rejects_corruption_before_restore() {
    let (mut s, st, _) = setup();
    st.lock()
        .unwrap()
        .files
        .insert("/nv/test".into(), vec![9; 35]);
    let dir = tempfile::tempdir().unwrap();
    let r = engine::snapshot(&mut s, &plan(), dir.path(), &mut |_| {}).unwrap();
    assert_eq!(r.files_seen, 2);
    let snap = engine::load_snapshot(dir.path()).unwrap();
    assert_eq!(snap.entries[0].0.metadata.as_ref().unwrap().mtime, 20);
    assert_eq!(snap.entries[1].1.as_ref().unwrap().len(), 128);
    std::fs::write(dir.path().join("000000.bin"), [1]).unwrap();
    assert_eq!(
        engine::load_snapshot(dir.path()).err().unwrap().code,
        "snapshotCorrupt"
    );
}
#[test]
fn ownership_busy_cancel_no_token_reset_and_default_gate() {
    let first = Operation::acquire().unwrap();
    assert_eq!(Operation::acquire().err().unwrap().code, "busy");
    assert!(crate::device_io::WriteOperation::acquire().is_err());
    first.cancel.store(true, Ordering::Release);
    assert!(first.cancel.load(Ordering::Acquire));
    drop(first);
    assert!(!Operation::acquire().unwrap().cancel.load(Ordering::Acquire));
    let other_engine = crate::device_io::WriteOperation::acquire().unwrap();
    assert_eq!(Operation::acquire().err().unwrap().code, "busy");
    drop(other_engine);
    if !cfg!(feature = "efs-write") {
        assert_eq!(gate().unwrap_err().code, "disabled");
    }
}

#[test]
fn rollback_roundtrip_restores_file_item_full_nv_and_removes_new_file() {
    let (mut s, st, _) = setup();
    let original_nv: Vec<u8> = (0..128).map(|n| n as u8).collect();
    {
        let mut state = st.lock().unwrap();
        state.files.insert("/nv/test".into(), vec![9; 35]);
        state.files.insert("/nv/item".into(), vec![7; 3]);
        state.metadata.insert("/nv/item".into(), (511, 15));
        state.nv.insert(562, original_nv.clone());
    }
    let mut plan = plan();
    plan.entries.push(manifest::Entry {
        name: "nv/item__309_F".into(),
        target: Target::Efs {
            path: "/nv/item".into(),
            mode: 777,
            entry_type: 15,
        },
        data: vec![1, 2, 3, 4],
    });
    plan.entries.push(manifest::Entry {
        name: "nv/new".into(),
        target: Target::Efs {
            path: "/nv/new".into(),
            mode: 777,
            entry_type: 0,
        },
        data: vec![5; 9],
    });
    let dir = tempfile::tempdir().unwrap();
    engine::snapshot(&mut s, &plan, dir.path(), &mut |_| {}).unwrap();
    engine::upload(&mut s, &plan, &mut |_| {}).unwrap();
    assert!(engine::verify(&mut s, &plan, &mut |_| {}).unwrap().ok);
    let restore = engine::load_snapshot(dir.path()).unwrap();
    let report = engine::rollback(&mut s, &restore, &mut |_| {}).unwrap();
    assert_eq!(report.files_seen, 4);
    assert!(s.cleanup().is_empty());
    let state = st.lock().unwrap();
    assert_eq!(state.files["/nv/test"], vec![9; 35]);
    assert_eq!(state.files["/nv/item"], vec![7; 3]);
    assert_eq!(state.metadata["/nv/item"], (511, 15));
    assert_eq!(state.nv[&562], original_nv);
    assert!(!state.files.contains_key("/nv/new"));
    assert!(state.handles.is_empty());
    assert_eq!(
        state
            .requests
            .iter()
            .filter(|r| r[0] == 0x4b && r[2] == 38)
            .count(),
        2
    );
}
#[test]
fn inactive_nv_is_recorded_as_no_prior_value_and_left_on_rollback() {
    // 실기기 XQ-DQ44: 스냅샷의 NV 읽기가 NV_NOTACTIVE(5)였다
    let (mut s, st, _) = setup();
    st.lock().unwrap().nv_inactive_missing = true;
    let plan = plan();
    let dir = tempfile::tempdir().unwrap();
    let out = engine::snapshot(&mut s, &plan, dir.path(), &mut |_| {}).unwrap();
    assert!(out.warnings.iter().any(|w| w.code == "nvInactiveBefore" && w.target == "NvItem__00000562"));
    engine::upload(&mut s, &plan, &mut |_| {}).unwrap();
    assert!(engine::verify(&mut s, &plan, &mut |_| {}).unwrap().ok);
    let restore = engine::load_snapshot(dir.path()).unwrap();
    let writes_before = st.lock().unwrap().requests.iter().filter(|r| r[0] == 0x27).count();
    engine::rollback(&mut s, &restore, &mut |_| {}).unwrap();
    let state = st.lock().unwrap();
    // 비활성으로 되돌릴 방법이 없으므로 NV는 다시 쓰지 않고, 파일은 원래대로(없음) 되돌린다
    assert_eq!(state.requests.iter().filter(|r| r[0] == 0x27).count(), writes_before);
    assert!(!state.files.contains_key("/nv/test"));
}

struct Replay {
    rx: std::io::Cursor<Vec<u8>>,
}
impl Read for Replay {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.rx.read(b)
    }
}
impl Write for Replay {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn replay(frames: Vec<Vec<u8>>) -> Session<Replay> {
    Session::new(
        Replay {
            rx: std::io::Cursor::new(
                frames
                    .into_iter()
                    .flat_map(|r| hdlc::encode(&r, false))
                    .collect(),
            ),
        },
        Arc::new(AtomicBool::new(false)),
        Duration::from_millis(50),
    )
}
#[test]
fn reply_matching_checks_subsystem_opcode_descriptor_offset_nv_id_and_sequence() {
    let req = wire::words(4, &[7, 1, 5]);
    let mut correct = wire::words(4, &[7, 5, 1, 0]);
    correct.push(1);
    let mut wrong_sub = correct.clone();
    wrong_sub[1] = 20;
    let mut wrong_op = correct.clone();
    wrong_op[2] = 5;
    let mut session = replay(vec![
        vec![0x10, 1],
        wrong_sub,
        wrong_op,
        wire::words(4, &[8, 5, 1, 0]),
        wire::words(4, &[7, 6, 1, 0]),
        correct.clone(),
    ]);
    assert_eq!(session.request(&req).unwrap(), correct);
    let mut nv = vec![0x26, 0x32, 2];
    nv.resize(133, 0);
    let mut wrong = nv.clone();
    wrong[1] = 5;
    assert_eq!(
        replay(vec![wrong, nv.clone()])
            .request(&wire::nv_read(562))
            .unwrap(),
        nv
    );
    let req = wire::words(12, &[2, 3]);
    let r = wire::words(12, &[2, 3, 0]);
    assert_eq!(
        replay(vec![wire::words(12, &[2, 2, 0]), r.clone()])
            .request(&req)
            .unwrap(),
        r
    );
    let mut req = wire::efs(48);
    req.extend(1u16.to_le_bytes());
    wire::path(&mut req, "/x").unwrap();
    let mut right = wire::efs(48);
    right.extend(1u16.to_le_bytes());
    right.extend([0; 8]);
    let mut wrong = right.clone();
    wrong[4] = 2;
    assert_eq!(
        replay(vec![wrong, right.clone()]).request(&req).unwrap(),
        right
    );
}
#[test]
fn device_errors_put_status_counts_and_malformed_responses_fail_closed() {
    let req = wire::efs(1);
    let reject = [vec![0x13], req.clone()].concat();
    assert_eq!(
        replay(vec![reject]).request(&req).unwrap_err().code,
        "unsupported"
    );
    assert_eq!(
        replay(vec![vec![0x4b, 19]]).request(&req).unwrap_err().code,
        "malformed"
    );
    assert_eq!(
        replay(vec![wire::words(15, &[13])])
            .stat("/x")
            .unwrap_err()
            .status,
        Some(13)
    );
    // PUT error uses u16 at byte 6, not generic EFS status at byte 4.
    let stat = wire::words(15, &[2]);
    let mkdir = wire::words(9, &[6]);
    let mut put = wire::efs(38);
    put.extend(777u16.to_le_bytes());
    put.extend(13u16.to_le_bytes());
    put.extend(1u16.to_le_bytes());
    assert_eq!(
        replay(vec![stat.clone(), mkdir.clone(), put])
            .write_file("/nv/item", 777, 15, &[1])
            .unwrap_err()
            .status,
        Some(13)
    );
    // 스냅샷 stat 모드(아이템 종류 비트 포함)로 되돌릴 때도 PUT·응답 대조는 권한 비트만 쓴다
    let mut ok = wire::efs(38);
    ok.extend(0o777u16.to_le_bytes());
    ok.extend(0u16.to_le_bytes());
    ok.extend(1u16.to_le_bytes());
    let mut d = replay(vec![stat.clone(), mkdir.clone(), ok, wire::words(48, &[0])]);
    let put_err = d.write_file("/nv/item", 0o160777, 15, &[1]).err();
    assert!(put_err.as_ref().map_or(true, |e| e.code != "malformed"), "{put_err:?}");
    // 쓴 바이트 수가 0이면(XQ-DQ44 실측) 리드백에 맡기고, 0이 아닌데 길이와 다르면 실패
    let mut put = wire::efs(38);
    put.extend(777u16.to_le_bytes());
    put.extend(0u16.to_le_bytes());
    put.extend(5u16.to_le_bytes());
    assert_eq!(
        replay(vec![stat, mkdir, put])
            .write_file("/nv/item", 777, 15, &[1])
            .unwrap_err()
            .code,
        "malformed"
    );
    assert_eq!(
        replay(vec![wire::words(9, &[17])])
            .mkdir("/nv", 777)
            .unwrap_err()
            .status,
        Some(17)
    );
}
#[test]
fn all_stock_inventory_balance_manifest_coverage_and_empty_nv_compatibility() {
    let stock: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/stock-presets.json")).unwrap();
    let mut balance_nv = 0;
    let mut performance_nv = 0;
    let mut balance_files = 0;
    for preset in stock {
        let entries = preset["entries"].as_array().unwrap();
        let nv = entries
            .iter()
            .filter(|e| {
                e["name"]
                    .as_str()
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .starts_with("NvItem__")
            })
            .count();
        if preset["set"] == "SonyEFS" {
            balance_nv += nv;
            balance_files += entries.len();
            let dir = tempfile::tempdir().unwrap();
            for entry in entries {
                let path = dir.path().join(entry["name"].as_str().unwrap());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, vec![0; entry["size"].as_u64().unwrap() as usize]).unwrap();
            }
            let plan = Plan::load(dir.path()).unwrap();
            assert_eq!(plan.entries.len(), entries.len());
            let skipped = engine::warnings(&plan)
                .iter()
                .filter(|w| w.code == "emptyNvSkipped")
                .count();
            assert_eq!(
                skipped,
                if preset["folder"]
                    .as_str()
                    .unwrap()
                    .starts_with("XPERIAsonyKT")
                {
                    2
                } else {
                    0
                }
            );
            assert_eq!(
                plan.active()
                    .filter(|e| matches!(e.target, Target::Nv { .. }))
                    .count(),
                nv - skipped
            );
        } else {
            performance_nv += nv;
        }
    }
    assert_eq!((balance_files, balance_nv, performance_nv), (723, 94, 189));
}
#[test]
#[ignore = "Read-only original bundle required through EFS_STOCK_ROOT"]
fn original_balance_hashes_match_pinned_manifest_without_changing_assets() {
    let root = std::env::var("EFS_STOCK_ROOT").expect("Set EFS_STOCK_ROOT to original bundle root");
    let data: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/stock-presets.json")).unwrap();
    let root = std::path::Path::new(&root).join("util/SonyEFS");
    let mut count = 0;
    for carrier in std::fs::read_dir(root).unwrap() {
        for folder in std::fs::read_dir(carrier.unwrap().path()).unwrap() {
            let path = folder.unwrap().path();
            if !path.is_dir() {
                continue;
            }
            let plan = Plan::load_approved(&path).unwrap();
            let inventory = data
                .iter()
                .find(|p| {
                    p["set"] == "SonyEFS"
                        && p["folder"] == path.file_name().unwrap().to_str().unwrap()
                })
                .unwrap();
            assert_eq!(
                plan.entries.len(),
                inventory["entries"].as_array().unwrap().len()
            );
            count += 1;
        }
    }
    assert_eq!(count, 8);
    // Real approved contents under another carrier/slot name must fail, not just any known hash.
    let root_bundle = std::path::PathBuf::from(std::env::var("EFS_STOCK_ROOT").unwrap());
    let source =
        Plan::load_approved(&root_bundle.join("util/SonyEFS/XPERIA-SKT/XPERIAsSKT1")).unwrap();
    let clone_root = tempfile::tempdir().unwrap();
    let wrong_name = clone_root.path().join("XPERIAsonyKT1");
    for entry in source.entries {
        let local = wrong_name.join(entry.name);
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(local, entry.data).unwrap();
    }
    assert_eq!(
        Plan::load_approved(&wrong_name).unwrap_err().code,
        "unapprovedPreset"
    );
    let util = std::path::Path::new(&std::env::var("EFS_STOCK_ROOT").unwrap()).join("util/SonyEFS");
    for second in ["XPERIA-KT/XPERIAsonyKT2", "XPERIA-LGU/XPERIAsonyLGU2"] {
        let plans = [
            Plan::load_approved(&util.join("XPERIA-SKT/XPERIAsSKT1")).unwrap(),
            Plan::load_approved(&util.join(second)).unwrap(),
        ];
        assert_eq!(
            manifest::validate_set(&plans).unwrap_err().code,
            "presetConflict"
        );
    }
    manifest::validate_set(&[
        Plan::load_approved(&util.join("XPERIA-SKT/XPERIAsSKT1")).unwrap(),
        Plan::load_approved(&util.join("XPERIA-SKT/XPERIAsSKT2")).unwrap(),
    ])
    .unwrap();
}

#[test]
fn stock_mixed_carriers_reject_conflicting_global_nv_before_phone_access() {
    let stock: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/stock-presets.json")).unwrap();
    let stock_plan = |name: &str| {
        let preset = stock
            .iter()
            .find(|p| p["set"] == "SonyEFS" && p["folder"] == name)
            .unwrap();
        Plan {
            sha256: "stock".into(),
            entries: preset["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|e| {
                    let name = e["name"].as_str().unwrap();
                    let id = name.strip_prefix("NvItem__")?.parse::<u16>().unwrap();
                    Some(manifest::Entry {
                        name: name.into(),
                        target: Target::Nv { id },
                        // Content hashes are equality proxies in the inventory regression.
                        // The opt-in read-only test below validates actual stock bytes too.
                        data: hex::decode(e["sha256"].as_str().unwrap()).unwrap(),
                    })
                })
                .collect(),
        }
    };
    for second in ["XPERIAsonyKT2", "XPERIAsonyLGU2"] {
        let plans = [stock_plan("XPERIAsSKT1"), stock_plan(second)];
        let err = manifest::validate_set(&plans).unwrap_err();
        assert_eq!(err.code, "presetConflict");
        assert!(err.message.contains("NV 71"));
    }
    manifest::validate_set(&[stock_plan("XPERIAsSKT1"), stock_plan("XPERIAsSKT2")]).unwrap();
    assert!(manifest::validate_set(&[]).is_err());
    let mut left = plan();
    let mut right = plan();
    manifest::validate_set(&[left, right]).unwrap();
    left = plan();
    right = plan();
    right.entries[0].target = Target::Efs {
        path: "/nv/test".into(),
        mode: 511,
        entry_type: 0,
    };
    assert_eq!(
        manifest::validate_set(&[left, right]).unwrap_err().code,
        "presetConflict"
    );
}
