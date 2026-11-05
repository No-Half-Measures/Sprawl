use futures::channel::mpsc;

// The trial uses a separate instance identifier; slice 5 will unify it with Tauri.
#[cfg(windows)]
pub const IDENTIFIER: &str = "com.sprawl.app.gpui";
#[cfg(any(windows, test))]
const DATA_ID: usize = 1542;
#[cfg(any(windows, test))]
const MAX_BYTES: usize = 128 * 1024;

#[cfg(any(windows, test))]
pub fn parse_message(kind: usize, bytes: &[u8]) -> Option<Vec<String>> {
    if kind != DATA_ID || bytes.len() > MAX_BYTES || bytes.last() != Some(&0) {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..bytes.len() - 1]).ok()?;
    if text.contains('\0') {
        return None;
    }
    let mut parts = text.split('|');
    parts.next()?;
    let args: Vec<_> = parts.map(String::from).collect();
    (!args.is_empty()).then_some(args)
}

pub struct Instance {
    pub receiver: Option<mpsc::UnboundedReceiver<Vec<String>>>,
    #[cfg(windows)]
    mutex: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    window: usize,
    #[cfg(windows)]
    thread: Option<std::thread::JoinHandle<()>>,
}
#[cfg(not(windows))]
pub fn start(_: &[String]) -> Result<Option<Instance>, String> {
    Ok(Some(Instance { receiver: None }))
}
#[cfg(windows)]
pub use windows::start;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::*,
        System::{DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW, Threading::*},
        UI::WindowsAndMessaging::*,
    };
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    type Sender = mpsc::UnboundedSender<Vec<String>>;
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_CREATE => {
                    let create = &*(lp as *const CREATESTRUCTW);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                    0
                }
                WM_COPYDATA => {
                    if lp == 0 {
                        return 0;
                    }
                    // Windows marshals WM_COPYDATA's buffer for the duration of this call.
                    let data = ptr::read_unaligned(lp as *const COPYDATASTRUCT);
                    if data.dwData != DATA_ID
                        || data.cbData == 0
                        || data.cbData as usize > MAX_BYTES
                        || data.lpData.is_null()
                    {
                        return 0;
                    }
                    let bytes =
                        std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                    if let Some(args) = parse_message(data.dwData, bytes) {
                        let sender = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Sender;
                        if !sender.is_null() {
                            let _ = (*sender).unbounded_send(args);
                            return 1;
                        }
                    }
                    0
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }
    pub fn start(args: &[String]) -> Result<Option<Instance>, String> {
        let class = wide(&format!("{IDENTIFIER}-sic"));
        let title = wide(&format!("{IDENTIFIER}-siw"));
        let mutex_name = wide(&format!("{IDENTIFIER}-sim"));
        let mutex = unsafe { CreateMutexW(ptr::null(), 0, mutex_name.as_ptr()) };
        let error = unsafe { GetLastError() };
        if mutex.is_null() && error != ERROR_ACCESS_DENIED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if error == ERROR_ALREADY_EXISTS || (mutex.is_null() && error == ERROR_ACCESS_DENIED) {
            let mut hwnd = ptr::null_mut();
            for _ in 0..100 {
                hwnd = unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
                if !hwnd.is_null() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let result = if hwnd.is_null() {
                Err("The first Sprawl instance is unavailable.".into())
            } else {
                let mut pid = 0;
                unsafe {
                    GetWindowThreadProcessId(hwnd, &mut pid);
                    AllowSetForegroundWindow(pid);
                }
                let payload = format!(
                    "{}|{}\0",
                    std::env::current_dir().unwrap_or_default().display(),
                    args.join("|")
                );
                if payload.len() > MAX_BYTES {
                    Err("Launch arguments are too large.".into())
                } else {
                    let data = COPYDATASTRUCT {
                        dwData: DATA_ID,
                        cbData: payload.len() as u32,
                        lpData: payload.as_ptr() as _,
                    };
                    let mut response = 0;
                    let sent = unsafe {
                        SendMessageTimeoutW(
                            hwnd,
                            WM_COPYDATA,
                            0,
                            &data as *const _ as isize,
                            SMTO_ABORTIFHUNG,
                            5000,
                            &mut response,
                        )
                    };
                    if sent == 0 || response != 1 {
                        Err("Could not forward launch arguments.".into())
                    } else {
                        Ok(None)
                    }
                }
            };
            unsafe {
                if !mutex.is_null() {
                    CloseHandle(mutex);
                }
            }
            return result;
        }
        let (sender, receiver) = mpsc::unbounded();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let sender = Box::new(sender);
            unsafe {
                let module = GetModuleHandleW(ptr::null());
                let wc = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(procedure),
                    hInstance: module,
                    lpszClassName: class.as_ptr(),
                    ..Default::default()
                };
                if RegisterClassExW(&wc) == 0 {
                    let _ = ready_tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    title.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    module,
                    (&*sender as *const Sender).cast(),
                );
                if hwnd.is_null() {
                    let _ = ready_tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let _ = ready_tx.send(Ok(hwnd as usize));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                UnregisterClassW(class.as_ptr(), module);
            }
        });
        match ready_rx.recv().map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(window) => Ok(Some(Instance {
                receiver: Some(receiver),
                mutex,
                window,
                thread: Some(thread),
            })),
            Err(e) => {
                unsafe {
                    CloseHandle(mutex);
                }
                let _ = thread.join();
                Err(e)
            }
        }
    }
}
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                self.window as _,
                windows_sys::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                0,
                0,
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.mutex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_untrusted_messages() {
        assert_eq!(
            parse_message(DATA_ID, b"cwd|exe|D:\\a & b,\0").unwrap(),
            ["exe", "D:\\a & b,"]
        );
        for bytes in [
            b"".as_slice(),
            b"cwd",
            b"cwd\0",
            b"cwd|\xff\0",
            b"cwd|exe\0bad\0",
        ] {
            assert!(parse_message(DATA_ID, bytes).is_none());
        }
        assert!(parse_message(0, b"cwd|exe\0").is_none());
        assert!(parse_message(DATA_ID, &vec![0; MAX_BYTES + 1]).is_none());
        for n in 0..100 {
            let bytes = vec![255; n];
            assert!(parse_message(DATA_ID, &bytes).is_none());
        }
    }
}
