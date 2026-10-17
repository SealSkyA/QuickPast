// sxs-test.rs - reliable CreateActCtxW check.
// Usage:
//   sxs-test.exe <manifest-file>            -> file-based activation
//   sxs-test.exe <exe> resource             -> resource-based activation of
//                                              RT_MANIFEST id 1 lang 0x409
//                                              (exactly what the process
//                                              loader does at startup)
use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn CreateActCtxW(p: *const ACTCTXW) -> *mut c_void;
    fn ReleaseActCtx(h: *mut c_void);
    fn GetLastError() -> u32;
}

#[repr(C)]
struct ACTCTXW {
    cb_size: u32,
    flags: u32,
    source: *const u16,
    proc_arch: u16,
    lang: u16,
    asm_dir: *const u16,
    params: *const u16,
    hmodule: *mut c_void,
    resname: *const u16,
    restype: *const u16,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    let mut args = std::env::args();
    let path = args.nth(1).unwrap_or_else(|| die("usage: sxs-test.exe <manifest-or-exe> [resource]"));
    let mode = args.next().unwrap_or_default();
    let mut ac = ACTCTXW {
        cb_size: std::mem::size_of::<ACTCTXW>() as u32,
        flags: 0,
        source: std::ptr::null(),
        proc_arch: 0,
        lang: 0,
        asm_dir: std::ptr::null(),
        params: std::ptr::null(),
        hmodule: std::ptr::null_mut(),
        resname: std::ptr::null(),
        restype: std::ptr::null(),
    };
    if mode == "resource" {
        // resource-based: RT_MANIFEST (24), id 1, language 0x0409
        let src = wide(&path);
        ac.source = src.as_ptr();
        ac.lang = 0x0409;
        ac.resname = 1 as *const u16;   // MAKEINTRESOURCE
        ac.restype = 24 as *const u16;  // MAKEINTRESOURCE
    } else {
        let src = wide(&path);
        ac.source = src.as_ptr();
    }
    unsafe {
        let h = CreateActCtxW(&ac);
        if h.is_null() {
            let e = GetLastError();
            println!("ACTCTX_FAIL err={e} (0x{e:X})");
            std::process::exit(2);
        } else {
            println!("ACTCTX_OK");
            ReleaseActCtx(h);
        }
    }
}

fn die(m: &str) -> ! {
    eprintln!("{m}");
    std::process::exit(1);
}
