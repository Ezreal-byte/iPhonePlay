//! Cancellable overlapped named-pipe writes. Handles and OVERLAPPED storage
//! remain alive until each outstanding operation has completed or been canceled.
use super::SessionControl;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_OUTBOUND, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows::core::PCWSTR;

pub(super) struct OutputPipe {
    pub path: String,
    handle: HANDLE,
    event: HANDLE,
}

// The two handles have one owner. They are moved to one writer thread and never
// shared; every operation is completed before dropping that owner.
unsafe impl Send for OutputPipe {}

impl OutputPipe {
    pub fn create(kind: &str) -> Result<Self, String> {
        let path = format!(
            r"\\.\pipe\{}-{}-{kind}",
            super::unique_stem("iPhonePlay"),
            std::process::id()
        );
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide.as_ptr()),
                PIPE_ACCESS_OUTBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                1024 * 1024,
                0,
                0,
                None,
            )
        };
        if handle.is_invalid() {
            return Err(format!(
                "无法创建录制管道：{}",
                windows::core::Error::from_thread()
            ));
        }
        match unsafe { CreateEventW(None, true, false, None) } {
            Ok(event) => Ok(Self {
                path,
                handle,
                event,
            }),
            Err(error) => {
                let _ = unsafe { CloseHandle(handle) };
                Err(format!("无法创建管道事件：{error}"))
            }
        }
    }

    pub fn connect(&mut self, control: &Arc<SessionControl>) -> Result<(), String> {
        let mut operation = OVERLAPPED {
            hEvent: self.event,
            ..Default::default()
        };
        let result = unsafe { ConnectNamedPipe(self.handle, Some(&mut operation)) };
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(()),
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => self
                .finish(&mut operation, control, Duration::from_secs(20))
                .map(|_| ()),
            Err(error) => Err(format!("录制管道连接失败：{error}")),
        }
    }

    pub fn write_all(
        &mut self,
        mut data: &[u8],
        control: &Arc<SessionControl>,
    ) -> Result<(), String> {
        while !data.is_empty() {
            if control.aborting() {
                return Err("录制写入已中止".into());
            }
            unsafe { ResetEvent(self.event) }.map_err(|e| e.to_string())?;
            let mut operation = OVERLAPPED {
                hEvent: self.event,
                ..Default::default()
            };
            let mut transferred = 0;
            let chunk = &data[..data.len().min(1024 * 1024)];
            let result = unsafe {
                WriteFile(
                    self.handle,
                    Some(chunk),
                    Some(&mut transferred),
                    Some(&mut operation),
                )
            };
            match result {
                Ok(()) => {}
                Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                    transferred = self.finish(&mut operation, control, Duration::from_secs(120))?;
                }
                Err(error) => return Err(format!("录制写入失败：{error}")),
            }
            if transferred == 0 {
                return Err("编码器停止读取录制数据".into());
            }
            data = &data[transferred as usize..];
        }
        Ok(())
    }

    fn finish(
        &self,
        operation: &mut OVERLAPPED,
        control: &Arc<SessionControl>,
        timeout: Duration,
    ) -> Result<u32, String> {
        let start = Instant::now();
        loop {
            let wait = unsafe { WaitForSingleObject(self.event, 50) };
            if wait == WAIT_OBJECT_0 {
                let mut transferred = 0;
                unsafe { GetOverlappedResult(self.handle, operation, &mut transferred, false) }
                    .map_err(|e| format!("录制管道操作失败：{e}"))?;
                return Ok(transferred);
            }
            if wait != WAIT_TIMEOUT || control.aborting() || start.elapsed() >= timeout {
                // Cancellation is asynchronous. Reap completion before stack
                // OVERLAPPED and the caller's byte buffer go out of scope.
                let _ = unsafe { CancelIoEx(self.handle, Some(operation)) };
                let mut transferred = 0;
                let _ =
                    unsafe { GetOverlappedResult(self.handle, operation, &mut transferred, true) };
                return Err("编码器无响应，已中止管道操作并保留临时文件".into());
            }
        }
    }
}

impl Drop for OutputPipe {
    fn drop(&mut self) {
        // Do not DisconnectNamedPipe: it discards unread bytes. Closing a fully
        // written handle lets FFmpeg drain the pipe and observe EOF.
        let _ = unsafe { CloseHandle(self.handle) };
        let _ = unsafe { CloseHandle(self.event) };
    }
}
