#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::OnceLock;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::SystemServices::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::Globalization::*;

pub mod proxy;
mod exports;

static DLL_DIR: OnceLock<Vec<u16>> = OnceLock::new();

#[no_mangle]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut ()) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            let mut path = [0u16; 1024];
            let len = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) };
            if len > 0 {
                let mut end = len as usize;
                while end > 0 && path[end - 1] != '\\' as u16 {
                    end -= 1;
                }
                if end > 0 {
                    let dir = &path[..end];
                    DLL_DIR.set(dir.to_vec()).ok();
                }
            }
            unsafe {
                CreateThread(
                    std::ptr::null(),
                    0,
                    Some(init_thread),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                );
            }
            1
        }
        DLL_PROCESS_DETACH => {
            unsafe { proxy::cleanup_proxied_dll() };
            1
        }
        _ => 1,
    }
}

unsafe extern "system" fn init_thread(_param: *mut c_void) -> u32 {
    initialize();
    0
}

unsafe fn utf8_to_utf16(input: &[u8]) -> Vec<u16> {
    if input.is_empty() {
        return Vec::new();
    }
    let len = MultiByteToWideChar(
        CP_UTF8,
        0,
        input.as_ptr(),
        input.len() as i32,
        std::ptr::null_mut(),
        0,
    );
    if len <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; len as usize];
    let result = MultiByteToWideChar(
        CP_UTF8,
        0,
        input.as_ptr(),
        input.len() as i32,
        buf.as_mut_ptr(),
        len,
    );
    if result > 0 {
        buf.truncate(result as usize);
        buf
    } else {
        Vec::new()
    }
}

fn is_absolute_path(line: &[u8]) -> bool {
    if line.len() >= 2 && line[1] == b':' && (line[0] >= b'A' && line[0] <= b'Z' || line[0] >= b'a' && line[0] <= b'z') {
        return true;
    }
    if line.len() >= 2 && line[0] == b'\\' && line[1] == b'\\' {
        return true;
    }
    false
}

fn initialize() {
    let dir = match DLL_DIR.get() {
        Some(d) => d,
        None => return,
    };

    let mut load_path = dir.clone();
    load_path.extend_from_slice(&[
        'l' as u16, 'o' as u16, 'a' as u16, 'd' as u16,
        '.' as u16, 't' as u16, 'x' as u16, 't' as u16, 0,
    ]);

    let handle = unsafe {
        CreateFileW(
            load_path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        )
    };

    if handle == INVALID_HANDLE_VALUE || handle == 0 {
        return;
    }

    let mut buffer = [0u8; 4096];
    let mut bytes_read = 0u32;
    let mut file_content = Vec::new();

    loop {
        let success = unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut bytes_read,
                std::ptr::null_mut(),
            )
        };
        if success == 0 || bytes_read == 0 {
            break;
        }
        file_content.extend_from_slice(&buffer[..bytes_read as usize]);
    }

    unsafe { CloseHandle(handle) };

    let mut start = 0;
    while start < file_content.len() {
        let end = match file_content[start..].iter().position(|&b| b == b'\n') {
            Some(pos) => start + pos,
            None => file_content.len(),
        };
        let line = &file_content[start..end];
        let line = line.trim_ascii();

        if !line.is_empty() && !line.starts_with(b"#") {
            let line_utf16 = unsafe { utf8_to_utf16(line) };
            if line_utf16.is_empty() {
                start = end + 1;
                continue;
            }

            let full_path = if is_absolute_path(line) {
                let mut p = line_utf16;
                p.push(0);
                p
            } else {
                let mut p = dir.clone();
                p.extend_from_slice(&line_utf16);
                p.push(0);
                p
            };

            let is_exe = line_utf16.ends_with(&['.' as u16, 'e' as u16, 'x' as u16, 'e' as u16])
                || line_utf16.ends_with(&['.' as u16, 'E' as u16, 'X' as u16, 'E' as u16]);

            if is_exe {
                run_exe(&full_path);
            } else {
                load_dll(&full_path);
            }
        }
        start = end + 1;
    }
}

fn load_dll(path: &[u16]) {
    unsafe {
        LoadLibraryW(path.as_ptr());
    }
}

fn run_exe(path: &[u16]) {
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process: PROCESS_INFORMATION = std::mem::zeroed();

        CreateProcessW(
            path.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &mut startup,
            &mut process,
        );
        CloseHandle(process.hProcess);
        CloseHandle(process.hThread);
    }
}
