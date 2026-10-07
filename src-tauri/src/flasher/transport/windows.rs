//! GordonGate-style handle transport from pinned Newflasher's Windows path.
//! No automatic driver changes. No hardware command uses this transport yet.
use super::super::{error, Result};
use super::FlashTransport;
use std::{
    fs::{File, OpenOptions},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    time::Duration,
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*,
    Foundation::{
        CloseHandle, GetLastError, ERROR_IO_PENDING, ERROR_NO_MORE_ITEMS, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    Storage::FileSystem::{ReadFile, WriteFile},
    System::{
        Threading::{CreateEventW, WaitForSingleObject},
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn interface_paths() -> Result<Vec<String>> {
    let guid = windows_sys::core::GUID {
        data1: 0xa5dcbf10,
        data2: 0x6530,
        data3: 0x11d2,
        data4: [0x90, 0x1f, 0, 0xc0, 0x4f, 0xb9, 0x51, 0xed],
    };
    let raw = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if raw == INVALID_HANDLE_VALUE as isize {
        return Err(error("DRIVER", "Cannot enumerate Windows USB interfaces"));
    }
    let set = DeviceSet(raw);
    let mut paths = Vec::new();
    for index in 0..4096 {
        let mut interface: SP_DEVICE_INTERFACE_DATA = unsafe { std::mem::zeroed() };
        interface.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        if unsafe {
            SetupDiEnumDeviceInterfaces(set.0, std::ptr::null(), &guid, index, &mut interface)
        } == 0
        {
            if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                return Ok(paths);
            }
            return Err(error("DRIVER", "Cannot enumerate Windows interface"));
        }
        let mut required = 0u32;
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                std::ptr::null_mut(),
                0,
                &mut required,
                std::ptr::null_mut(),
            );
        }
        if required < std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32
            || required > 32768
        {
            return Err(error("DRIVER", "Invalid Windows interface detail size"));
        }
        // Alignment is at least that of the native detail structure on x86/x64.
        let mut storage = vec![0u64; (required as usize).div_ceil(8)];
        let detail = storage
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        unsafe {
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                detail,
                required,
                &mut required,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(error("DRIVER", "Cannot read Windows interface detail"));
        }
        let offset = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
        if required as usize > storage.len() * 8 || (required as usize) < offset + 2 {
            return Err(error("DRIVER", "Invalid returned interface detail size"));
        }
        let count = (required as usize - offset) / 2;
        let wide = unsafe { std::slice::from_raw_parts((*detail).DevicePath.as_ptr(), count) };
        let length = wide
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| error("DRIVER", "Windows device path is unterminated"))?;
        let path = String::from_utf16(&wide[..length])
            .map_err(|_| error("DRIVER", "Invalid Windows device path"))?;
        if flash_path(&path) {
            paths.push(path);
        }
    }
    Err(error(
        "DRIVER",
        "Windows interface enumeration limit reached",
    ))
}

fn flash_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.starts_with("\\\\?\\usb#") && !path.contains('\0') && path.contains("vid_0fce&pid_b00b#")
}

pub struct WindowsFlashTransport {
    file: File,
}
impl WindowsFlashTransport {
    /// The caller must separately bind interface selection to the expected physical phone.
    pub fn open(path: &str) -> Result<Self> {
        if !flash_path(path) {
            return Err(error(
                "INTERFACE",
                "Not the pinned Sony Flash mode interface",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(3)
            .custom_flags(0x40000000)
            .open(path)
            .map_err(|_| error("DRIVER", "Cannot open Sony Flash mode interface"))?;
        Ok(Self { file })
    }
    fn io(&mut self, buffer: &mut [u8], write: bool, timeout: Duration) -> Result<usize> {
        if timeout.is_zero() || buffer.is_empty() || buffer.len() > u32::MAX as usize {
            return Err(error("IO", "Invalid Flash mode I/O request"));
        }
        let event = Event(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) });
        if event.0.is_null() {
            return Err(error("IO", "Cannot create Flash mode I/O event"));
        }
        let handle = self.file.as_raw_handle() as HANDLE;
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event.0;
        let accepted = unsafe {
            if write {
                WriteFile(
                    handle,
                    buffer.as_ptr(),
                    buffer.len() as u32,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    handle,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            }
        };
        if accepted == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err(error("IO", "Flash mode I/O could not start"));
        }
        let wait = unsafe {
            WaitForSingleObject(
                event.0,
                timeout.as_millis().clamp(1, u32::MAX as u128 - 1) as u32,
            )
        };
        if wait != WAIT_OBJECT_0 {
            // Cancellation is a request, not completion. Keep buffer/event/OVERLAPPED alive until
            // the driver completes or cancels the operation, even when completion is delayed.
            unsafe {
                CancelIoEx(handle, &overlapped);
            }
            let mut transferred = 0;
            unsafe {
                GetOverlappedResult(handle, &overlapped, &mut transferred, 1);
            }
            return Err(error(
                "IO_TIMEOUT",
                "Flash mode I/O stopped; device result requires revalidation",
            ));
        }
        let mut transferred = 0;
        if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
            return Err(error("IO", "Flash mode I/O completion failed"));
        }
        if transferred as usize > buffer.len() {
            return Err(error("IO", "Flash mode I/O length overflow"));
        }
        Ok(transferred as usize)
    }
}
impl FlashTransport for WindowsFlashTransport {
    fn write(&mut self, data: &[u8], timeout: Duration) -> Result<usize> {
        // Owned backing memory remains valid through cancellation/completion.
        let mut buffer = data.to_vec();
        self.io(&mut buffer, true, timeout)
    }
    fn read_response(&mut self, limit: usize, timeout: Duration) -> Result<Vec<u8>> {
        if limit == 0 || limit > 16 * 1024 {
            return Err(error("RESPONSE", "Invalid response buffer limit"));
        }
        let mut buffer = vec![0; limit];
        let length = self.io(&mut buffer, false, timeout)?;
        buffer.truncate(length);
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_path_filter_is_sony_flashmode_specific_without_opening_hardware() {
        assert!(flash_path("\\\\?\\usb#vid_0fce&pid_b00b#test#{guid}"));
        for path in [
            "C:/file",
            "\\\\?\\usb#vid_0fce&pid_adde#test#{guid}",
            "\\\\?\\usb#vid_18d1&pid_b00b#test#{guid}",
            "\\\\?\\usb#vid_0fce&pid_b00b0#test#{guid}",
        ] {
            assert!(!flash_path(path));
        }
    }
}
