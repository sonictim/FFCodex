use std::ffi::{c_char, c_int, c_void};

pub fn get_v() -> String {
    get_version()
}

#[link(name = "chromaprint", kind = "static")]
unsafe extern "C" {
    pub fn chromaprint_get_version() -> *const c_char;

    pub fn chromaprint_new(algorithm: c_int) -> *mut c_void;
    pub fn chromaprint_free(ctx: *mut c_void);

    pub fn chromaprint_start(ctx: *mut c_void, sample_rate: c_int, num_channels: c_int) -> c_int;
    pub fn chromaprint_feed(ctx: *mut c_void, data: *const i16, size: c_int) -> c_int;
    pub fn chromaprint_finish(ctx: *mut c_void) -> c_int;

    pub fn chromaprint_get_fingerprint(ctx: *mut c_void, fingerprint: *mut *mut c_char) -> c_int;
    pub fn chromaprint_dealloc(ptr: *mut c_void);
}

pub const CHROMAPRINT_ALGORITHM_TEST1: c_int = 0;
pub const CHROMAPRINT_ALGORITHM_TEST2: c_int = 1;
pub const CHROMAPRINT_ALGORITHM_TEST3: c_int = 2;
pub const CHROMAPRINT_ALGORITHM_DEFAULT: c_int = CHROMAPRINT_ALGORITHM_TEST2;

// Safe Rust wrapper for Chromaprint
pub struct Chromaprint {
    ctx: *mut c_void,
}

impl Chromaprint {
    pub fn new(algorithm: c_int) -> Self {
        let ctx = unsafe { chromaprint_new(algorithm) };
        if ctx.is_null() {
            panic!("Failed to create Chromaprint context");
        }
        Chromaprint { ctx }
    }

    pub fn default() -> Self {
        Self::new(CHROMAPRINT_ALGORITHM_DEFAULT)
    }

    pub fn start(&self, sample_rate: i32, num_channels: i32) -> bool {
        unsafe { chromaprint_start(self.ctx, sample_rate, num_channels) == 1 }
    }

    pub fn feed(&self, data: &[i16]) -> bool {
        unsafe { chromaprint_feed(self.ctx, data.as_ptr(), data.len() as c_int) == 1 }
    }

    pub fn finish(&self) -> bool {
        unsafe { chromaprint_finish(self.ctx) == 1 }
    }

    pub fn get_fingerprint(&self) -> Option<String> {
        let mut fingerprint: *mut c_char = std::ptr::null_mut();
        let result = unsafe { chromaprint_get_fingerprint(self.ctx, &mut fingerprint) };

        if result == 1 && !fingerprint.is_null() {
            let c_str = unsafe { std::ffi::CStr::from_ptr(fingerprint) };
            let fingerprint_str = c_str.to_string_lossy().into_owned();
            unsafe { chromaprint_dealloc(fingerprint as *mut c_void) };
            Some(fingerprint_str)
        } else {
            None
        }
    }
}

impl Drop for Chromaprint {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { chromaprint_free(self.ctx) };
        }
    }
}

// Safe convenience function to get the library version
pub fn get_version() -> String {
    unsafe {
        let c_str = std::ffi::CStr::from_ptr(chromaprint_get_version());
        c_str.to_string_lossy().into_owned()
    }
}
