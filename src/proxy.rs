#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{atomic::AtomicPtr, OnceLock};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::SystemInformation::*;

static SYSTEM_DLL: OnceLock<AtomicPtr<HMODULE>> = OnceLock::new();
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

unsafe fn load_proxied_dll(dll_name: &str) -> Option<HMODULE> {
    if let Some(dll) = SYSTEM_DLL.get() {
        return Some(*dll.load(Ordering::Relaxed));
    }

    let mut sys_dir = [0u16; 260];
    let len = GetSystemDirectoryW(sys_dir.as_mut_ptr(), sys_dir.len() as u32);
    if len == 0 {
        return None;
    }

    let dll_name_wide: Vec<u16> = dll_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut full_path = Vec::new();
    full_path.extend_from_slice(&sys_dir[..len as usize]);
    full_path.push('\\' as u16);
    full_path.extend_from_slice(&dll_name_wide);

    let dll = LoadLibraryW(full_path.as_ptr());
    if !dll.is_null() {
        SYSTEM_DLL.set(AtomicPtr::new(Box::into_raw(Box::new(dll)))).ok();
        Some(dll)
    } else {
        None
    }
}

pub unsafe fn cleanup_proxied_dll() {
    if SHUTTING_DOWN.swap(true, Ordering::Relaxed) {
        return;
    }

    if let Some(dll) = SYSTEM_DLL.get() {
        FreeLibrary(*dll.load(Ordering::Relaxed));
        SYSTEM_DLL.set(AtomicPtr::new(Box::into_raw(Box::new(std::ptr::null_mut())))).ok();
    }
}

pub unsafe fn get_proxied_func(dll_name: &str, func_name: &str) -> Option<unsafe extern "system" fn()> {
    let dll = load_proxied_dll(dll_name)?;
    let func_name_cstr = CString::new(func_name).ok()?;
    let proc_addr = GetProcAddress(dll, func_name_cstr.as_ptr());
    if proc_addr.is_null() {
        None
    } else {
        Some(std::mem::transmute(proc_addr))
    }
}

#[macro_export]
macro_rules! proxy_function {
    ($dll:literal, $name:ident, ($($param:ident: $param_type:ty),*), $ret_type:ty, $default:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn $name($($param: $param_type),*) -> $ret_type {
            type FuncType = unsafe extern "system" fn($($param_type),*) -> $ret_type;
            if let Some(func) = crate::proxy::get_proxied_func($dll, stringify!($name)) {
                let func: FuncType = std::mem::transmute(func);
                func($($param),*)
            } else {
                $default
            }
        }
    };
    ($dll:literal, $name:ident, ($($param:ident: $param_type:ty),*), $ret_type:ty, fallback: $fallback_fn:ident($($fallback_arg:ident),*)) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn $name($($param: $param_type),*) -> $ret_type {
            type FuncType = unsafe extern "system" fn($($param_type),*) -> $ret_type;
            if let Some(func) = crate::proxy::get_proxied_func($dll, stringify!($name)) {
                let func: FuncType = std::mem::transmute(func);
                func($($param),*)
            } else {
                $fallback_fn($($fallback_arg),*)
            }
        }
    };
                               }
