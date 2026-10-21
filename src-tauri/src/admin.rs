use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Settings {
    pub always_elevate: bool,
}

pub fn read_settings(path: &Path) -> Settings {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn write_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp)?;
    serde_json::to_writer(&mut file, settings).map_err(io::Error::other)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)
}

#[cfg(not(windows))]
pub fn elevated() -> bool {
    false
}

#[cfg(windows)]
pub fn elevated() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = std::ptr::null_mut();
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0;
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let result = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        result != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(windows)]
pub(super) fn quote_arg(arg: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let mut quoted = vec![b'"' as u16];
    let mut slashes = 0;
    // Windows doubles backslashes before quotes and the closing quote.
    for unit in arg.encode_wide() {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let count = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        quoted.extend(std::iter::repeat_n(b'\\' as u16, count));
        quoted.push(unit);
        slashes = 0;
    }
    quoted.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    quoted.push(b'"' as u16);
    quoted
}

#[cfg(not(windows))]
pub fn relaunch() -> Result<(), String> {
    Err("Restarting as administrator is only supported on Windows.".into())
}

#[cfg(windows)]
pub fn relaunch() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::ERROR_CANCELLED,
        UI::{
            Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    let exe = std::env::current_exe()
        .map_err(|e| format!("Could not locate the DiskMapper executable: {e}"))?;
    let dir = std::env::current_dir()
        .map_err(|e| format!("Could not read the working directory: {e}"))?;
    let exe: Vec<_> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let dir: Vec<_> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut parameters = vec![];
    for arg in std::env::args_os()
        .skip(1)
        .filter(|arg| !arg.to_string_lossy().starts_with("--elevated-parent="))
        .chain([
            "--elevated".into(),
            format!("--elevated-parent={}", std::process::id()).into(),
        ])
    {
        parameters.extend(quote_arg(&arg));
        parameters.push(b' ' as u16);
    }
    parameters.push(0);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait for the launch before exiting this process; report errors in the app.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: windows_sys::core::w!("runas"),
        lpFile: exe.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: dir.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            return Err("Administrator access was declined.".into());
        }
        return Err(format!("Could not restart as administrator: {error}"));
    }
    Ok(())
}

#[cfg(windows)]
pub fn wait_for_parent() -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };

    let parent = std::env::args()
        .find_map(|arg| arg.strip_prefix("--elevated-parent=")?.parse::<u32>().ok());
    let Some(parent) = parent else { return Ok(()) };
    // The elevated child must not forward to the instance that is about to exit.
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
    if process.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(format!(
                "Could not wait for the previous DiskMapper instance: {error}"
            ))
        };
    }
    let result = unsafe { WaitForSingleObject(process, 30_000) };
    unsafe { CloseHandle(process) };
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err("The previous DiskMapper instance did not exit.".into())
    }
}
