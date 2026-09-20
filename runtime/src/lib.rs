use std::ffi::CStr;
use std::io::{self, Write};

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print(s: *const u8) {
    if s.is_null() {
        return;
    }

    let bytes = unsafe { CStr::from_ptr(s.cast()).to_bytes() };
    let text = String::from_utf8_lossy(bytes);

    print!("{text}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println(s: *const u8) {
    if s.is_null() {
        println!();
        return;
    }

    let bytes = unsafe { CStr::from_ptr(s.cast()).to_bytes() };
    let text = String::from_utf8_lossy(bytes);

    println!("{text}");
}
