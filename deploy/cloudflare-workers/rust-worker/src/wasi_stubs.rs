//! Definitions for the WASI imports wasi-libc references.
//!
//! The C/C++ codecs (mozjpeg, jpegli, libwebp) are compiled against the
//! wasi-sdk sysroot because wasm32-unknown-unknown has no libc. wasi-libc
//! reaches the host through `__imported_wasi_snapshot_preview1_*`
//! symbols; defining them here lets lld resolve them inside the module,
//! so the final wasm has no `wasi_snapshot_preview1` imports for
//! wasm-bindgen (or the Workers runtime) to choke on.
//!
//! Only stdio and the clock do anything: there is no filesystem, no
//! environment, and no argv in a Worker.
#![allow(clippy::missing_safety_doc)]

const ESUCCESS: i32 = 0;
const EBADF: i32 = 8;
const ENOSYS: i32 = 52;

#[repr(C)]
struct Ciovec {
    buf: *const u8,
    len: usize,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_write(
    fd: i32,
    iovs: *const Ciovec,
    iovs_len: i32,
    nwritten: *mut usize,
) -> i32 {
    let iovs = unsafe { std::slice::from_raw_parts(iovs, iovs_len as usize) };
    let mut msg = Vec::new();
    for v in iovs {
        msg.extend_from_slice(unsafe { std::slice::from_raw_parts(v.buf, v.len) });
    }
    unsafe { *nwritten = msg.len() };
    if fd == 1 || fd == 2 {
        worker::console_log!("[c fd{fd}] {}", String::from_utf8_lossy(&msg).trim_end());
        ESUCCESS
    } else {
        EBADF
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_sizes_get(
    count: *mut usize,
    size: *mut usize,
) -> i32 {
    unsafe {
        *count = 0;
        *size = 0;
    }
    ESUCCESS
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_environ_get(_: *mut u8, _: *mut u8) -> i32 {
    ESUCCESS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_time_get(
    _id: i32,
    _precision: i64,
    out: *mut u64,
) -> i32 {
    unsafe { *out = (worker::Date::now().as_millis() as u64) * 1_000_000 };
    ESUCCESS
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_proc_exit(code: i32) -> ! {
    panic!("C code called exit({code})");
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_close(_: i32) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_fdstat_get(_: i32, _: *mut u8) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_filestat_get(_: i32, _: *mut u8) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_seek(
    _: i32,
    _: i64,
    _: i32,
    _: *mut u64,
) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_read(
    _: i32,
    _: *const Ciovec,
    _: i32,
    _: *mut usize,
) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_prestat_get(_: i32, _: *mut u8) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
extern "C" fn __imported_wasi_snapshot_preview1_fd_prestat_dir_name(
    _: i32,
    _: *mut u8,
    _: i32,
) -> i32 {
    EBADF
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
extern "C" fn __imported_wasi_snapshot_preview1_path_open(
    _: i32,
    _: i32,
    _: *const u8,
    _: i32,
    _: i32,
    _: i64,
    _: i64,
    _: i32,
    _: *mut i32,
) -> i32 {
    ENOSYS
}
