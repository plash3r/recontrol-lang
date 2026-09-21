use std::ffi::CStr;
use std::io::{self, Write};

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print(s: *const u8) {
    if s.is_null() { return; }
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

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print_i8(value: i8) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println_i8(value: i8) {
    println!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_print_i32(value: i32) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_println_i32(value: i32) {
    println!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_check_bounds_i32(index: i32, length: i32) {
    if index < 0 || index >= length {
        eprintln!("rcl: array index {index} is out of bounds for length {length}");
        let _ = io::stderr().flush();
        std::process::exit(1);
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn rcl_check_divisor(is_zero: i32) {
    if is_zero != 0 {
        eprintln!("rcl: integer division or remainder by zero");
        let _ = io::stderr().flush();
        std::process::exit(1);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rcl_check_div_overflow(is_overflow: i32) {
    if is_overflow != 0 {
        eprintln!("rcl: signed integer division overflow (minimum value divided by -1)");
        let _ = io::stderr().flush();
        std::process::exit(1);
    }
}
