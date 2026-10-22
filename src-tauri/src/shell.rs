pub fn launch_path(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut paths = args
        .into_iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| normalize_path(&arg))
        .filter(|arg| !arg.is_empty());
    let path = paths.next()?;
    paths.next().is_none().then_some(path)
}

fn normalize_path(path: &str) -> String {
    let path = path.trim_matches('"');
    if path.len() == 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.ends_with(':') {
        format!("{path}\\")
    } else {
        path.to_string()
    }
}

#[cfg(any(windows, test))]
fn command(exe: &str, background: bool) -> String {
    let target = if background { "%V" } else { "%1" };
    format!("\"{exe}\" \"{target}\"")
}

#[cfg(not(windows))]
pub fn context_menu_enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_context_menu(_enabled: bool) -> Result<(), String> {
    Err("Explorer's right-click menu is only supported on Windows.".into())
}

#[cfg(windows)]
pub use windows::{allow_scan_requests, context_menu_enabled, set_context_menu};

#[cfg(windows)]
mod windows {
    use std::{io, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
            RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
        },
        UI::{
            Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify},
            WindowsAndMessaging::{
                ChangeWindowMessageFilterEx, FindWindowW, GetWindowThreadProcessId, MSGFLT_ALLOW,
                WM_COPYDATA,
            },
        },
    };

    const KEYS: [(&str, bool); 3] = [
        (r"Software\Classes\Directory\shell\Sprawl", false),
        (
            r"Software\Classes\Directory\Background\shell\Sprawl",
            true,
        ),
        (r"Software\Classes\Drive\shell\Sprawl", false),
    ];

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }

    pub fn allow_scan_requests(identifier: &str) -> Result<(), String> {
        // The single-instance plugin's hidden window receives launches from unelevated Explorer.
        let window = unsafe {
            FindWindowW(
                wide(format!("{identifier}-sic")).as_ptr(),
                wide(format!("{identifier}-siw")).as_ptr(),
            )
        };
        let mut process = 0;
        unsafe { GetWindowThreadProcessId(window, &mut process) };
        if process != std::process::id() {
            return Err("The single-instance window is unavailable.".into());
        }
        if unsafe {
            ChangeWindowMessageFilterEx(window, WM_COPYDATA, MSGFLT_ALLOW, ptr::null_mut())
        } == 0
        {
            return Err(format!(
                "Could not receive Explorer scan requests: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn check(result: u32) -> Result<(), String> {
        if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "Could not update Explorer's right-click menu: {}",
                io::Error::from_raw_os_error(result as i32)
            ))
        }
    }

    fn create(path: &str) -> Result<Key, String> {
        let mut key = ptr::null_mut();
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        })?;
        Ok(Key(key))
    }

    fn write(key: &Key, name: &str, value: &str) -> Result<(), String> {
        let value = wide(value);
        check(unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        })
    }

    fn matches_command(path: &str, expected: &str) -> bool {
        let expected = wide(expected);
        let mut value = vec![0u16; expected.len()];
        let mut size = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(format!(r"{path}\command")).as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                value.as_mut_ptr().cast(),
                &mut size,
            )
        };
        result == ERROR_SUCCESS && size as usize == expected.len() * 2 && value == expected
    }

    pub fn context_menu_enabled() -> bool {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let exe = exe.to_string_lossy();
        KEYS.iter()
            .all(|(path, background)| matches_command(path, &super::command(&exe, *background)))
    }

    pub fn set_context_menu(enabled: bool) -> Result<(), String> {
        let result = (|| {
            if enabled {
                let exe = std::env::current_exe()
                    .map_err(|e| format!("Could not locate the Sprawl executable: {e}"))?;
                let exe = exe.to_string_lossy();
                for (path, background) in KEYS {
                    let key = create(path)?;
                    write(&key, "", "Scan with Sprawl")?;
                    write(&key, "Icon", &format!("\"{exe}\",0"))?;
                    write(
                        &create(&format!(r"{path}\command"))?,
                        "",
                        &super::command(&exe, background),
                    )?;
                }
            } else {
                for (path, _) in KEYS {
                    let result = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(path).as_ptr()) };
                    if result != ERROR_FILE_NOT_FOUND {
                        check(result)?;
                    }
                }
            }
            Ok(())
        })();
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                ptr::null(),
                ptr::null(),
            )
        };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_path_and_ignores_flags() {
        for (args, expected) in [
            (vec![r"D:\Photos"], r"D:\Photos"),
            (
                vec![
                    "--elevated",
                    r"D:\Photos",
                    "--elevated-parent=42",
                    "--other",
                ],
                r"D:\Photos",
            ),
            (vec![r#""D:\Photos""#], r"D:\Photos"),
            (vec![r#"D:\Photos""#], r"D:\Photos"),
            (vec![r#""D:\Photos with spaces""#], r"D:\Photos with spaces"),
            (vec![r"\\server\share"], r"\\server\share"),
        ] {
            assert_eq!(
                launch_path(args.into_iter().map(String::from)),
                Some(expected.to_string())
            );
        }
        for args in [
            vec![],
            vec!["--elevated"],
            vec![""],
            vec![r"D:\Photos", r"E:\Music"],
        ] {
            assert_eq!(launch_path(args.into_iter().map(String::from)), None);
        }
    }

    #[test]
    fn normalizes_quoted_paths_and_drive_roots() {
        for path in [r"C:\", r#""C:\""#, r#"C:""#, r#"C:\""#] {
            assert_eq!(normalize_path(path), r"C:\");
        }
        assert_eq!(normalize_path(r#""D:\Photos""#), r"D:\Photos");
        assert_eq!(normalize_path(r#"D:\Photos""#), r"D:\Photos");
        assert_eq!(
            normalize_path(r#""\\server\share name""#),
            r"\\server\share name"
        );
    }

    #[test]
    fn quotes_executable_and_explorer_targets() {
        let exe = r"C:\Program Files\Sprawl\Sprawl.exe";
        assert_eq!(
            command(exe, false),
            r#""C:\Program Files\Sprawl\Sprawl.exe" "%1""#
        );
        assert_eq!(
            command(exe, true),
            r#""C:\Program Files\Sprawl\Sprawl.exe" "%V""#
        );
    }
}
