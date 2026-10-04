//! fastboot 트랜스포트 — 프로토콜 로직(protocol.rs)을 하드웨어에서 분리하는 트레이트와 rusb 구현.
//! rusb 구현은 컴파일만 확인(실기기 테스트 금지 — 사용자 승인 2026-10-03), 프로토콜 검증은 FakeTransport로.

use std::time::Duration;

/// fastboot 인터페이스 (AOSP 표준) — class 0xFF / subclass 0x42 / protocol 0x03
pub const FB_CLASS: u8 = 0xFF;
pub const FB_SUBCLASS: u8 = 0x42;
pub const FB_PROTOCOL: u8 = 0x03;

const SONY_VID: u16 = 0x0FCE;
/// 응답 대기(초) — DATA 본문 전송 중에는 적용하지 않는다(§9-3 유한 처리)
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
/// 기기가 오래 일하는 명령(언락 초기화·플래시 기록·본문 수신 확인)의 응답 대기.
/// 10초 안에 끝나지 않아도 기기는 계속 진행하므로, 짧게 끊으면 성공을 실패로 기록하게 된다.
pub const LONG_RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);

/// 트레이트 — 명령 전송·응답 프레임 수신·데이터 본문 전송. 단위 테스트는 가짜로 대체.
pub trait FastbootTransport {
    /// ASCII 명령 전송(≤64바이트)
    fn write_command(&mut self, cmd: &str) -> Result<(), String>;
    /// 응답 프레임 1개 수신 — 반환: (status 4바이트, 페이로드) 합친 원본 버퍼의 길이.
    /// 버퍼는 최대 256바이트(fastboot 응답 상한). timeout은 프레임 1개당 대기 시간.
    fn read_frame(&mut self, buf: &mut [u8; 256], timeout: Duration) -> Result<usize, String>;
    /// DATA 본문 전송 — 호스트→기기 bulk OUT
    fn write_data(&mut self, data: &[u8]) -> Result<(), String>;
}

/// rusb fastboot 장치 — 열림 시 인터페이스 클레임까지 수행
pub struct RusbTransport {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
    ep_out: u8,
    ep_in: u8,
    iface_no: u8,
}

impl RusbTransport {
    /// fastboot 모드 Sony 장치를 정확히 1대 찾아 연다(§9-3 — 여러 대면 거부, 없으면 안내)
    pub fn open() -> Result<Self, String> {
        let devices = rusb::devices().map_err(|e| format!("USB 장치 목록 조회 실패: {e}"))?;
        let mut matches: Vec<rusb::Device<rusb::GlobalContext>> = Vec::new();
        for dev in devices.iter() {
            let Ok(desc) = dev.device_descriptor() else {
                continue;
            };
            if desc.vendor_id() == SONY_VID && has_fastboot_interface(&dev, &desc) {
                matches.push(dev);
            }
        }
        match matches.len() {
            0 => Err(
                "fastboot 모드 기기를 찾을 수 없습니다 — 폰을 부트로더 모드로 진입시켜 주세요"
                    .into(),
            ),
            1 => Self::claim(&matches[0]),
            n => Err(format!(
                "fastboot 모드 장치가 {n}대 연결되어 있습니다 — 작업할 폰만 남겨 주세요"
            )),
        }
    }

    fn claim(dev: &rusb::Device<rusb::GlobalContext>) -> Result<Self, String> {
        let handle = dev.open().map_err(|e| format!("USB 장치 열기 실패: {e}"))?;
        // 인터페이스 번호·엔드포인트 탐색
        let desc = dev.device_descriptor().map_err(|e| e.to_string())?;
        let mut iface_no = None;
        let mut ep_out = None;
        let mut ep_in = None;
        let mut configuration = None;
        let mut alternate = None;
        'outer: for cfg_idx in 0..desc.num_configurations() {
            let Ok(cfg) = dev.config_descriptor(cfg_idx) else {
                continue;
            };
            for iface in cfg.interfaces() {
                for alt in iface.descriptors() {
                    if alt.class_code() == FB_CLASS
                        && alt.sub_class_code() == FB_SUBCLASS
                        && alt.protocol_code() == FB_PROTOCOL
                    {
                        let mut out = None;
                        let mut input = None;
                        for ep in alt.endpoint_descriptors() {
                            if ep.transfer_type() != rusb::TransferType::Bulk {
                                continue;
                            }
                            if ep.direction() == rusb::Direction::Out {
                                out = Some(ep.address());
                            } else {
                                input = Some(ep.address());
                            }
                        }
                        if out.is_none() || input.is_none() {
                            continue;
                        }
                        iface_no = Some(iface.number());
                        ep_out = out;
                        ep_in = input;
                        configuration = Some(cfg.number());
                        alternate = Some(alt.setting_number());
                        break 'outer;
                    }
                }
            }
        }
        let iface_no = iface_no.ok_or("fastboot 인터페이스를 찾지 못했습니다")?;
        let configuration = configuration.ok_or("fastboot USB 설정을 찾지 못했습니다")?;
        if handle.active_configuration().map_err(|e| e.to_string())? != configuration {
            handle
                .set_active_configuration(configuration)
                .map_err(|e| e.to_string())?;
        }
        // 지원하는 플랫폼에서는 release 시 커널 드라이버도 자동 복구한다.
        match handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(e) => return Err(format!("USB 드라이버 분리 설정 실패: {e}")),
        }
        handle
            .claim_interface(iface_no)
            .map_err(|e| format!("fastboot 인터페이스 클레임 실패(드라이버 확인 필요): {e}"))?;
        if let Err(e) = handle.set_alternate_setting(iface_no, alternate.unwrap_or(0)) {
            let _ = handle.release_interface(iface_no);
            return Err(format!("fastboot USB 대체 설정 실패: {e}"));
        }
        Ok(Self {
            handle,
            ep_out: ep_out.ok_or("bulk OUT 엔드포인트가 없습니다")?,
            ep_in: ep_in.ok_or("bulk IN 엔드포인트가 없습니다")?,
            iface_no,
        })
    }
}

impl FastbootTransport for RusbTransport {
    fn write_command(&mut self, cmd: &str) -> Result<(), String> {
        if !cmd.is_ascii() || cmd.is_empty() || cmd.len() > 64 {
            return Err(format!("fastboot 명령이 너무 깁니다({}바이트)", cmd.len()));
        }
        let written = self
            .handle
            .write_bulk(self.ep_out, cmd.as_bytes(), RESPONSE_TIMEOUT)
            .map_err(|e| format!("명령 전송 실패: {e}"))?;
        if written != cmd.len() {
            return Err(format!(
                "명령 일부만 전송되었습니다({written}/{})",
                cmd.len()
            ));
        }
        Ok(())
    }

    fn read_frame(&mut self, buf: &mut [u8; 256], timeout: Duration) -> Result<usize, String> {
        match self.handle.read_bulk(self.ep_in, buf, timeout) {
            Ok(n) => Ok(n),
            Err(rusb::Error::Timeout) => Err("fastboot 응답 시간 초과".into()),
            Err(e) => Err(format!("fastboot 응답 수신 실패: {e}")),
        }
    }

    fn write_data(&mut self, data: &[u8]) -> Result<(), String> {
        // 청크 단위 전송 — max-download-size 상한은 프로토콜 층에서 검사했다
        const CHUNK: usize = 512 * 1024; // fastboot 표준 max chunk
        for piece in data.chunks(CHUNK) {
            write_all_data(piece, |remaining| {
                self.handle
                    .write_bulk(self.ep_out, remaining, Duration::from_secs(60))
                    .map_err(|e| format!("데이터 전송 실패: {e}"))
            })?;
        }
        Ok(())
    }
}

impl Drop for RusbTransport {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(self.iface_no);
    }
}

fn write_all_data(
    mut data: &[u8],
    mut write: impl FnMut(&[u8]) -> Result<usize, String>,
) -> Result<(), String> {
    while !data.is_empty() {
        let n = write(data)?;
        if n == 0 || n > data.len() {
            return Err("데이터 전송이 진행되지 않았습니다".into());
        }
        data = &data[n..];
    }
    Ok(())
}

fn has_fastboot_interface(
    dev: &rusb::Device<rusb::GlobalContext>,
    desc: &rusb::DeviceDescriptor,
) -> bool {
    for cfg_idx in 0..desc.num_configurations() {
        let Ok(cfg) = dev.config_descriptor(cfg_idx) else {
            continue;
        };
        for iface in cfg.interfaces() {
            for alt in iface.descriptors() {
                if alt.class_code() == FB_CLASS
                    && alt.sub_class_code() == FB_SUBCLASS
                    && alt.protocol_code() == FB_PROTOCOL
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_data_writes_preserve_every_byte() {
        let mut sent = vec![];
        write_all_data(b"abcdefg", |data| {
            let n = data.len().min(2);
            sent.extend_from_slice(&data[..n]);
            Ok(n)
        })
        .unwrap();
        assert_eq!(sent, b"abcdefg");
        assert!(write_all_data(b"x", |_| Ok(0)).is_err());
        assert!(write_all_data(b"x", |_| Err("timeout".into())).is_err());
    }
}
