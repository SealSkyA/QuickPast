// reslink-test.rs - minimal exe that links the windres-shim resource object
// and SELF-VERIFIES the embedded PE resource tree via FindResourceExW.
// Exit codes: 0 = resources queryable (manifest + icon group + icon),
// 3 = resource tree broken (false positive link), 2 = module load failure.

use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn FindResourceExW(h: *mut c_void, ty: *const u16, name: *const u16, lang: u16) -> *mut c_void;
    fn SizeofResource(h: *mut c_void, res: *mut c_void) -> u32;
    fn FreeLibrary(h: *mut c_void) -> i32;
}

fn main() {
    println!("resource-link-test");
    let me = std::env::current_exe().unwrap();
    let wide: Vec<u16> = me
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let h = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 2) }; // LOAD_LIBRARY_AS_DATAFILE
    if h.is_null() {
        eprintln!("FATAL: LoadLibraryExW failed");
        std::process::exit(2);
    }
    // MAKEINTRESOURCE semantics: a small integer passed as LPWSTR is a resource id.
    let ok = |ty: u16, id: u16, lang: u16| unsafe {
        let r = FindResourceExW(h, ty as *const u16, id as *const u16, lang);
        !r.is_null() && SizeofResource(h, r) > 0
    };
    let manifest = ok(24, 1, 0x0409); // RT_MANIFEST id 1 (SxS activation)
    let group = ok(14, 32512, 0x0409); // RT_GROUP_ICON id 32512 (explorer icon)
    let icon = ok(3, 2, 0x0409); // RT_ICON id 2 (first icon blob)
    println!("resource-check manifest={manifest} group={group} icon={icon}");
    if !(manifest && group && icon) {
        eprintln!("FATAL: resource tree not queryable via FindResourceExW");
        std::process::exit(3);
    }
    unsafe {
        FreeLibrary(h);
    }
    println!("RESOURCE_OK");
}
