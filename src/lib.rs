use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use winapi::shared::minwindef::{BOOL, DWORD, HMODULE, LPVOID, TRUE, FALSE};
use winapi::um::libloaderapi::{GetModuleFileNameW, LoadLibraryW};
use winapi::um::winnt::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use winapi::um::processthreadsapi::{CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW};
use winapi::um::handleapi::CloseHandle;
use winapi::um::errhandlingapi::GetLastError;

pub mod proxy;
mod exports;

static DLL_PATH: OnceLock<PathBuf> = OnceLock::new();

fn log_message(msg: &str) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("proxy_debug.log")
        .and_then(|mut f| {
            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            f.write_all(format!("[{}] {}\n", timestamp, msg).as_bytes())
        });
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
unsafe extern "system" fn DllMain(
    module: HMODULE,
    call_reason: DWORD,
    _reserved: LPVOID,
) -> BOOL {
    match call_reason {
        DLL_PROCESS_ATTACH => {
            log_message("DLL_PROCESS_ATTACH started");

            let dll_path = {
                let mut buffer = [0u16; 1024];
                let len = GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32);
                if len == 0 {
                    log_message("Failed to get module filename");
                    return FALSE;
                }
                let path = OsString::from_wide(&buffer[..len as usize]);
                let path = PathBuf::from(path);
                match path.parent() {
                    Some(parent) => parent.to_owned(),
                    None => {
                        log_message("Failed to get parent directory");
                        return FALSE;
                    }
                }
            };

            log_message(&format!("DLL_PATH set to: {:?}", dll_path));

            if DLL_PATH.set(dll_path).is_err() {
                log_message("Failed to set DLL_PATH (already set)");
                return FALSE;
            }

            initialize();

            TRUE
        }
        DLL_PROCESS_DETACH => {
            log_message("DLL_PROCESS_DETACH");
            unsafe { proxy::cleanup_proxied_dll() };
            TRUE
        }
        _ => TRUE,
    }
}

fn initialize() {
    log_message("initialize() started");

    let dll_path = match DLL_PATH.get() {
        Some(path) => {
            log_message(&format!("DLL_PATH: {:?}", path));
            path
        }
        None => {
            log_message("DLL_PATH not set!");
            return;
        }
    };

    let load_path = dll_path.join("load.txt");
    log_message(&format!("Looking for load.txt at: {:?}", load_path));

    if !load_path.exists() {
        log_message("load.txt not found! Creating default...");
        let _ = File::create(&load_path).and_then(|mut f| {
            f.write_all(b"# Proxy Loader Configuration\n")
                .and_then(|_| f.write_all(b"# Add DLL or EXE files to load, one per line\n"))
                .and_then(|_| f.write_all(b"# Example:\n"))
                .and_then(|_| f.write_all(b"# myplugin.dll\n"))
                .and_then(|_| f.write_all(b"# tool.exe\n"))
        });
        log_message("Default load.txt created");
        return;
    }

    let file = match File::open(&load_path) {
        Ok(f) => f,
        Err(e) => {
            log_message(&format!("Failed to open load.txt: {}", e));
            return;
        }
    };

    let reader = BufReader::new(file);

    for line in reader.lines() {
        if let Ok(line) = line {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            log_message(&format!("Processing: {}", line));

            let file_path = if Path::new(line).is_absolute() {
                PathBuf::from(line)
            } else {
                dll_path.join(line)
            };

            log_message(&format!("Full path: {:?}", file_path));

            if !file_path.exists() {
                log_message(&format!("File not found: {:?}", file_path));
                continue;
            }

            if let Some(ext) = file_path.extension() {
                if ext == "exe" {
                    log_message(&format!("Running EXE: {:?}", file_path));
                    run_exe(&file_path);
                    continue;
                }
            }

            log_message(&format!("Loading DLL: {:?}", file_path));
            match load_dll(&file_path) {
                Ok(_) => log_message(&format!("Successfully loaded: {:?}", file_path)),
                Err(e) => log_message(&format!("Failed to load: {:?} - {}", file_path, e)),
            }
        }
    }
}

fn load_dll(dll_path: &Path) -> Result<(), Box<dyn Error>> {
    let path_wide: Vec<u16> = dll_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    
    unsafe {
        let lib = LoadLibraryW(path_wide.as_ptr());
        if lib.is_null() {
            let error = GetLastError();
            return Err(format!("Failed to load library, error code: {}", error).into());
        }
    }

    Ok(())
}

fn run_exe(exe_path: &Path) {
    let path_wide: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut startup_info: STARTUPINFOW = std::mem::zeroed();
        startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process_info: PROCESS_INFORMATION = std::mem::zeroed();

        let result = CreateProcessW(
            path_wide.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut startup_info,
            &mut process_info,
        );

        if result != 0 {
            CloseHandle(process_info.hProcess);
            CloseHandle(process_info.hThread);
            log_message(&format!("EXE started successfully: {:?}", exe_path));
        } else {
            let error = GetLastError();
            log_message(&format!("Failed to start EXE: {:?}, error: {}", exe_path, error));
        }
    }
}
