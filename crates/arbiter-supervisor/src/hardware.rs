//! Best-effort capacity detection. Missing readings stay unknown.
use std::path::Path;

pub fn capacity(path: &Path) -> (Option<u64>, Option<u64>) {
    platform(path)
}

#[cfg(windows)]
fn platform(path: &Path) -> (Option<u64>, Option<u64>) {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    struct Memory {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        page_total: u64,
        page_available: u64,
        virtual_total: u64,
        virtual_available: u64,
        extended: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(value: *mut Memory) -> i32;
        fn GetDiskFreeSpaceExW(path: *const u16, available: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let mut m = Memory {
        length: std::mem::size_of::<Memory>() as u32,
        load: 0,
        total: 0,
        available: 0,
        page_total: 0,
        page_available: 0,
        virtual_total: 0,
        virtual_available: 0,
        extended: 0,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0;
    // Valid initialized structs and NUL-terminated path remain alive for the calls.
    unsafe {
        (
            (GlobalMemoryStatusEx(&mut m) != 0).then_some(m.total),
            (GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) != 0)
                .then_some(free),
        )
    }
}
#[cfg(unix)]
fn platform(path: &Path) -> (Option<u64>, Option<u64>) {
    use std::os::unix::ffi::OsStrExt;
    let ram = unsafe {
        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
        let size = libc::sysconf(libc::_SC_PAGESIZE);
        (pages > 0 && size > 0).then(|| (pages as u64).saturating_mul(size as u64))
    };
    let disk = std::ffi::CString::new(path.as_os_str().as_bytes()).ok().and_then(|p| unsafe {
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if libc::statvfs(p.as_ptr(), stat.as_mut_ptr()) != 0 {
            None
        } else {
            let s = stat.assume_init();
            // The field widths differ between Unix systems (u64 on Linux,
            // u32 for f_bavail on macOS), so the cast is needed on some.
            #[allow(clippy::unnecessary_cast)]
            let (avail, block) = (s.f_bavail as u64, s.f_frsize as u64);
            Some(avail.saturating_mul(block))
        }
    });
    (ram, disk)
}
#[cfg(not(any(windows, unix)))]
fn platform(_: &Path) -> (Option<u64>, Option<u64>) {
    (None, None)
}
/// Includes Windows junctions/reparse points as well as symbolic links.
pub fn is_link(m: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_type().is_symlink() || m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}
