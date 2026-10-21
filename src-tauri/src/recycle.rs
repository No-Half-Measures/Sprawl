use crate::scanner::{self, Kind};
use std::{fs, path::Path};

const CHANGED: &str = "This item has changed. Rescan to refresh it.";
const PROTECTED: &str = "Windows needs this folder, so DiskMapper won't delete it.";

fn normalized(path: &str) -> String {
    let path = path.replace('/', "\\").to_lowercase();
    let prefix = if path.starts_with(r"\\") { r"\\" } else { "" };
    let parts: Vec<_> = path.split('\\').filter(|part| !part.is_empty()).collect();
    format!("{prefix}{}", parts.join("\\"))
}

pub fn protected(path: &str, directories: &[String]) -> bool {
    let path = normalized(path);
    let bytes = path.as_bytes();
    (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || (path.starts_with(r"\\") && path[2..].split('\\').count() <= 2)
        || directories.iter().any(|directory| {
            let directory = normalized(directory);
            !path.is_empty()
                && (directory == path
                    || directory
                        .strip_prefix(&path)
                        .is_some_and(|rest| rest.starts_with('\\')))
        })
}

pub fn check(path: &Path, kind: Kind) -> Result<(), String> {
    let mut directories: Vec<_> = [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "USERPROFILE",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .collect();
    if let Ok(profile) = std::env::var("USERPROFILE")
        && let Some(parent) = Path::new(&profile).parent()
    {
        directories.push(parent.to_string_lossy().into_owned());
    }
    if protected(&path.to_string_lossy(), &directories) {
        return Err(PROTECTED.into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| CHANGED.to_string())?;
    if scanner::excluded(&metadata)
        || !matches!(
            (kind, metadata.is_dir(), metadata.is_file()),
            (Kind::Dir, true, false) | (Kind::File, false, true)
        )
    {
        return Err(CHANGED.into());
    }
    for ancestor in path.ancestors().skip(1) {
        if scanner::excluded(&fs::symlink_metadata(ancestor).map_err(|_| CHANGED.to_string())?) {
            return Err(CHANGED.into());
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn outcome(
    name: &str,
    permanent: bool,
    code: i32,
    aborted: bool,
    exists: bool,
    partial: bool,
) -> Result<(), String> {
    if !exists {
        return Ok(());
    }
    if partial {
        return Err(if permanent {
            "Some items couldn't be deleted. Rescan to see what's left."
        } else {
            "Some items couldn't be moved to the Recycle Bin. Rescan to see what's left."
        }
        .into());
    }
    if aborted {
        return Err("Delete cancelled.".into());
    }
    Err(if permanent {
        format!("Couldn't delete {name}.")
    } else {
        format!("Couldn't move {name} to the Recycle Bin. (Shell code {code})")
    })
}

#[cfg(not(windows))]
pub fn delete_item(
    _path: &Path,
    _name: &str,
    _owner: usize,
    permanent: bool,
) -> Result<(), String> {
    Err(if permanent {
        "Deleting is only supported on Windows."
    } else {
        "The Recycle Bin is only supported on Windows."
    }
    .into())
}

#[cfg(windows)]
fn source(path: &Path, permanent: bool) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    let path = crate::scanner::strip_verbatim_prefix(path);
    let mut source: Vec<_> = path.as_os_str().encode_wide().collect();
    if source.len() >= windows_sys::Win32::Foundation::MAX_PATH as usize {
        return Err(if permanent {
            "This path is too long to delete."
        } else {
            "This path is too long for the Recycle Bin."
        }
        .into());
    }
    if !path.is_absolute() || source.iter().any(|&unit| matches!(unit, 0 | 42 | 63)) {
        return Err(CHANGED.into());
    }
    source.extend([0, 0]);
    Ok(source)
}

#[cfg(windows)]
fn flags(permanent: bool) -> u16 {
    use windows_sys::Win32::UI::Shell::{
        FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_WANTNUKEWARNING,
    };
    (FOF_NOCONFIRMATION
        | FOF_NOERRORUI
        | if permanent {
            0
        } else {
            FOF_ALLOWUNDO | FOF_WANTNUKEWARNING
        }) as u16
}

#[cfg(windows)]
pub fn delete_item(path: &Path, name: &str, owner: usize, permanent: bool) -> Result<(), String> {
    use windows_sys::Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        UI::Shell::{FO_DELETE, SHFILEOPSTRUCTW, SHFileOperationW},
    };
    let source = source(path, permanent)?;
    let mut contents = vec![];
    let mut stack = vec![path.to_owned()];
    while let Some(path) = stack.pop() {
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.is_dir()
            && !scanner::excluded(&metadata)
            && let Ok(entries) = fs::read_dir(path)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                contents.push(path.clone());
                stack.push(path);
            }
        }
    }
    let initialized = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if initialized < 0 {
        return Err(if permanent {
            format!("Couldn't delete {name}.")
        } else {
            format!("Couldn't initialize the Recycle Bin. ({initialized:#x})")
        });
    }
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: owner as _,
        wFunc: FO_DELETE,
        pFrom: source.as_ptr(),
        fFlags: flags(permanent),
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    unsafe { CoUninitialize() };
    let missing = |path: &Path| {
        fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    };
    let exists = !missing(path);
    let partial = exists && contents.iter().any(|path| missing(path));
    outcome(
        name,
        permanent,
        code,
        operation.fAnyOperationsAborted != 0,
        exists,
        partial,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_paths_allow_contents_but_never_ancestors() {
        let directories = [
            r"C:\Windows",
            r"C:\Program Files",
            r"C:\Program Files (x86)",
            r"C:\ProgramData",
            r"C:\Users",
            r"C:\Users\Example",
            r"D:\System\Windows",
        ]
        .map(String::from);
        for path in [
            r"C:\",
            "z:/",
            "C:",
            r"c:/WINDOWS/",
            r"C:\Program Files",
            r"c:\PROGRAM FILES (X86)\",
            r"C:\ProgramData",
            r"C:\Users",
            r"c:/users/example/",
            r"c:\\USERS//example\\",
            r"D:\System",
            r"D:\System\Windows",
            r"\\server\share\",
        ] {
            assert!(protected(path, &directories), "{path}");
        }
        for path in [
            r"C:\Windows\Temp",
            r"C:\Program Files\App",
            r"C:\Program Files (x86)\App",
            r"C:\ProgramData\App",
            r"C:\Users\Example\Downloads",
            r"C:\Users\Other",
            r"C:\WindowsOld",
            r"D:\System\Windows\Temp",
            r"C:\Data",
            r"\\server\share\folder",
        ] {
            assert!(!protected(path, &directories), "{path}");
        }
    }

    #[test]
    fn shell_outcomes_require_the_path_to_be_gone() {
        for permanent in [false, true] {
            for code in [0, 5, 0x78] {
                for aborted in [false, true] {
                    for partial in [false, true] {
                        assert_eq!(
                            outcome("file", permanent, code, aborted, false, partial),
                            Ok(())
                        );
                    }
                    assert_eq!(
                        outcome("file", permanent, code, aborted, true, true).unwrap_err(),
                        if permanent {
                            "Some items couldn't be deleted. Rescan to see what's left."
                        } else {
                            "Some items couldn't be moved to the Recycle Bin. Rescan to see what's left."
                        }
                    );
                }
                assert_eq!(
                    outcome("file", permanent, code, true, true, false).unwrap_err(),
                    "Delete cancelled."
                );
                assert_eq!(
                    outcome("file", permanent, code, false, true, false).unwrap_err(),
                    if permanent {
                        "Couldn't delete file.".to_string()
                    } else {
                        format!("Couldn't move file to the Recycle Bin. (Shell code {code})")
                    }
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn shell_flags_only_allow_undo_and_nuke_warning_when_recycling() {
        use windows_sys::Win32::UI::Shell::{
            FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_WANTNUKEWARNING,
        };
        assert_eq!(flags(true), (FOF_NOCONFIRMATION | FOF_NOERRORUI) as u16);
        assert_eq!(
            flags(false),
            (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_NOERRORUI) as u16
        );
    }

    #[cfg(windows)]
    #[test]
    fn shell_source_is_absolute_short_and_double_terminated() {
        assert_eq!(
            source(Path::new(r"\\?\C:\data\file.txt"), false).unwrap(),
            "C:\\data\\file.txt\0\0".encode_utf16().collect::<Vec<_>>()
        );
        for permanent in [false, true] {
            assert!(source(Path::new("relative"), permanent).is_err());
            assert!(source(Path::new(r"C:\data\*"), permanent).is_err());
            for length in [260, 261] {
                assert_eq!(
                    source(
                        Path::new(&format!("C:\\{}", "x".repeat(length - 3))),
                        permanent
                    )
                    .unwrap_err(),
                    if permanent {
                        "This path is too long to delete."
                    } else {
                        "This path is too long for the Recycle Bin."
                    }
                );
            }
            assert_eq!(
                source(Path::new(&format!("C:\\{}", "x".repeat(256))), permanent)
                    .unwrap()
                    .len(),
                261
            );
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Recycles and permanently deletes temporary files using the Windows shell"]
    fn real_delete_modes() {
        for permanent in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("DiskMapper recycle test.txt");
            fs::write(&path, "recycle test").unwrap();
            delete_item(&path, "DiskMapper recycle test.txt", 0, permanent).unwrap();
            assert!(!path.exists());
        }
    }
}
