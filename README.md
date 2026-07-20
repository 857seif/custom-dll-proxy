# Custom DLL Proxy

> A lightweight, production-ready DLL proxying framework written in Rust. Drop it in, configure a text file, and hijack Windows DLL loads with full transparency to the host process.

---

## Table of Contents

- [What is DLL Proxying?](#what-is-dll-proxying)
- [How This Project Works](#how-this-project-works)
- [Project Structure](#project-structure)
- [The Code: A Deep Dive](#the-code-a-deep-dive)
  - [`lib.rs` — The Brain](#librs--the-brain)
  - [`proxy.rs` — The Middleman](#proxyrs--the-middleman)
  - [`exports.rs` — The Mask](#exportsrs--the-mask)
  - [`Cargo.toml` — The Blueprint](#cargotoml--the-blueprint)
  - [`load.txt` — The Trigger](#loadtxt--the-trigger)
- [Building](#building)
- [Usage](#usage)
- [Why Rust?](#why-rust)
- [Detection & Defense](#detection--defense)
- [Disclaimer](#disclaimer)

---

## What is DLL Proxying?

DLL Proxying is a sophisticated technique used in both offensive security and system compatibility scenarios. At its core, it involves placing a malicious (or custom) DLL in the search path of a legitimate executable. The executable loads *your* DLL instead of the real one. Your DLL then:

1. **Executes your payload** (e.g., loads more DLLs, spawns processes).
2. **Forwards all legitimate function calls** to the original DLL so the host application never crashes or behaves abnormally.

This is what makes proxying so stealthy compared to raw DLL hijacking — the victim process continues working perfectly, reducing the chance of raising suspicion.

> **Real-world context:** APT groups like APT29 and malware families like Dridex have famously abused DLL side-loading and proxying to evade EDR detection.

---

## How This Project Works

```
+---------------+     +------------------+     +----------------+
| Legit App.exe |---->| Your version.dll |---->| Real version.dll|
| (looks for    |     | (this project)   |     | (in System32)  |
|  version.dll) |     |                  |     |                |
+---------------+     | - Runs payload   |     +----------------+
                        | - Forwards calls |
                        +------------------+
```

1. You compile this project as `version.dll` (or any target DLL name).
2. Place it next to a legitimate executable that loads `version.dll`.
3. The app loads your proxy first.
4. Your proxy reads `load.txt` from the same directory and loads additional DLLs or runs executables.
5. All `version.dll` exports are seamlessly forwarded to the real `version.dll` in `System32`.

---

## Project Structure

```
.
├── Cargo.toml      # Rust build configuration
├── src/
│   ├── lib.rs      # DllMain, init thread, file parsing, payload execution
│   ├── proxy.rs    # System DLL loading, function forwarding, cleanup
│   └── exports.rs  # All exported functions proxied to the real DLL
└── load.txt        # Configuration file listing payloads to execute
```

---

## The Code: A Deep Dive

### `lib.rs` — The Brain

This is where everything starts. The `DllMain` entry point is the first thing Windows calls when your DLL is loaded into a process.

```rust
#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut ()) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // ... capture DLL directory, spawn init thread ...
        }
        DLL_PROCESS_DETACH => {
            unsafe { proxy::cleanup_proxied_dll() };
        }
        _ => 1,
    }
}
```

**Why spawn a thread in `DllMain`?**

`DllMain` runs under the loader lock — a global critical section in Windows. If you do anything complex inside it (like file I/O or `LoadLibrary`), you risk deadlocking the entire process. The classic solution is to create a new thread that does the heavy lifting after `DllMain` returns.

```rust
unsafe {
    CreateThread(
        std::ptr::null_mut(), 0,
        Some(init_thread),
        std::ptr::null_mut(), 0, std::ptr::null_mut(),
    );
}
```

**The `initialize()` function** reads `load.txt` line-by-line. Each line can be:
- A **relative path** — resolved against the DLL's directory.
- An **absolute path** — used as-is.
- A `.exe` — spawned with `CreateProcessW`.
- A `.dll` — loaded with `LoadLibraryW`.

Lines starting with `#` are treated as comments.

```rust
let is_exe = line_utf16.ends_with(&['.' as u16, 'e' as u16, 'x' as u16, 'e' as u16]);
if is_exe {
    run_exe(&full_path);
} else {
    load_dll(&full_path);
}
```

This simple text-based config makes the proxy incredibly flexible. You don't need to recompile to change payloads — just edit `load.txt`.

---

### `proxy.rs` — The Middleman

This module handles the *proxying* part of DLL proxying. Its job is to load the **real** system DLL and hand back function pointers when the host app asks for them.

```rust
unsafe fn load_proxied_dll(dll_name: &str) -> Option<HMODULE> {
    // 1. Get System32 path
    // 2. Append the real DLL name
    // 3. LoadLibraryW on the real DLL
    // 4. Cache the handle in a static
}
```

The `SYSTEM_DLL` static uses `OnceLock` for thread-safe, one-time initialization. This is a Rust idiom that replaces error-prone `InitOnceExecuteOnce` or raw atomics you'd write in C.

**The `proxy_function!` macro** is where the magic happens. It generates a no-mangle function that:
1. Looks up the real function via `GetProcAddress`.
2. If found, transmutes and calls it.
3. If not found, returns a default value or calls a fallback function.

```rust
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
}
```

This macro is *beautifully* Rust-y. It eliminates hundreds of lines of boilerplate you'd need in C. Each exported function becomes a one-liner in `exports.rs`.

---

### `exports.rs` — The Mask

This file defines every function that the host application expects to find in `version.dll`. If even one export is missing, the app will likely fail to start.

```rust
proxy_function!("version.dll", GetFileVersionInfoA,
    (filename: *const u8, handle: u32, len: u32, data: *mut ()), i32, 0);
proxy_function!("version.dll", GetFileVersionInfoSizeA,
    (filename: *const u8, handle: *mut u32), u32, 0);
// ... and so on for every export
```

Notice the `fallback:` variant for newer functions like `GetFileVersionInfoExA`:

```rust
proxy_function!("version.dll", GetFileVersionInfoExA,
    (flags: u32, filename: *const u8, handle: u32, len: u32, data: *mut ()),
    i32, fallback: GetFileVersionInfoA(filename, handle, len, data));
```

This is a smart compatibility trick. If the real `version.dll` on an older Windows version doesn't export `GetFileVersionInfoExA`, the proxy falls back to the older `GetFileVersionInfoA`. This keeps the host app stable across different Windows versions.

---

### `Cargo.toml` — The Blueprint

```toml
[lib]
name = "version"
crate-type = ["cdylib"]
```

Setting `crate-type = ["cdylib"]` tells Rust to compile as a C-compatible dynamic library (`.dll` on Windows). The `name = "version"` ensures the output file is `version.dll`.

The release profile is aggressively optimized for size and stealth:

```toml
[profile.release]
lto = true          # Link-time optimization
strip = true        # Strip symbols
opt-level = "z"     # Optimize for size
codegen-units = 1   # Single codegen unit for better LTO
panic = "abort"     # No unwinding overhead
```

This produces a tiny, symbol-free DLL that's harder to analyze statically.

---

### `load.txt` — The Trigger

```text
file1.dll
file2.dll
file1.exe
C:\Users\Desktop\file.exe
D:\Games\mods\file.dll
```

This is your mission control. When the proxy loads, it reads this file and:
- Loads `file1.dll` and `file2.dll` into the process.
- Spawns `file1.exe` and `C:\Users\Desktop\file.exe` as new processes.
- Loads `D:\Games\mods\file.dll` from an absolute path.

Because it's plain text, you can generate it dynamically during an engagement or modify it without touching the compiled binary.

---

## Building

### Prerequisites

- [Rust](https://rustup.rs/) (stable channel)
- Windows target: `rustup target add x86_64-pc-windows-gnu` (or `msvc`)
- For cross-compilation from Linux: `mingw-w64`

### Compile

```bash
# Native Windows (MSVC)
cargo build --release

# Cross-compile from Linux (MinGW)
cargo build --release --target x86_64-pc-windows-gnu
```

The output will be `target/release/version.dll`.

---

## Usage

1. **Identify a target application** that loads `version.dll` (or rename the project to match your target DLL).
2. **Rename the real DLL** in the app's directory (e.g., `version.dll` -> `version_real.dll`) *only if* the app ships its own copy. If it's loading from `System32`, you don't need to move anything — the proxy will find it there.
3. **Drop your proxy DLL** into the app's directory as `version.dll`.
4. **Create `load.txt`** in the same directory with your payloads.
5. **Run the application.**

---

## Why Rust?

You might be wondering: *"Why not just write this in C?"*

Fair question. Here's why Rust shines here:

| Feature | Rust | C |
|---|---|---|
| **Memory Safety** | No buffer overflows, use-after-free, or dangling pointers at compile time | Manual, error-prone |
| **Thread Safety** | `OnceLock`, `AtomicPtr` — safe by default | Raw atomics, easy to mess up |
| **Macros** | `proxy_function!` eliminates boilerplate | C macros are brittle and unsafe |
| **Modern Tooling** | `cargo`, `rustfmt`, `clippy` | Varies by project |
| **FFI** | Clean `unsafe` blocks with clear boundaries | Entire codebase is effectively unsafe |
| **Binary Size** | `opt-level = "z"` + LTO produces tiny binaries | Comparable, but harder to achieve safely |

In security tooling, reliability is everything. A crashing proxy DLL is a detection event. Rust's guarantees help you avoid the classic pitfalls that plague C-based implants.

---

## Detection & Defense

From a **blue team** perspective, DLL proxying is tricky but not invisible:

- **Unsigned DLLs in app directories:** Look for DLLs that don't match the publisher of the host EXE.
- **DLL load order anomalies:** EDRs can detect when a known executable loads a DLL from an unexpected path.
- **Export table analysis:** The proxy's exports will match the real DLL, but the binary itself will differ.
- **Behavioral signals:** Spawning child processes or loading additional DLLs from `DllMain` threads can trigger behavioral detections.

From a **red team** perspective, opsec considerations:

- **Sign your proxy DLL** if possible (legitimate code signing cert).
- **Use a DLL that the app *actually* needs** — don't proxy a DLL that never gets loaded.
- **Keep the payload light** — heavy crypto or network activity from a DLL thread is suspicious.
- **Clean up on detach** — the `DLL_PROCESS_DETACH` handler frees the proxied DLL to avoid leaks.

---

