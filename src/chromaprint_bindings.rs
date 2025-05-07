#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::ffi::{c_char, c_int, c_short, c_void};

pub type ChromaprintContext = *mut c_void;

// Added 'unsafe' keyword here
unsafe extern "C" {
    pub fn chromaprint_new(algorithm: c_int) -> ChromaprintContext;
    pub fn chromaprint_free(ctx: ChromaprintContext);
    pub fn chromaprint_start(
        ctx: ChromaprintContext,
        sample_rate: c_int,
        num_channels: c_int,
    ) -> c_int;
    pub fn chromaprint_feed(ctx: ChromaprintContext, data: *const c_short, size: c_int) -> c_int;
    pub fn chromaprint_finish(ctx: ChromaprintContext) -> c_int;
    pub fn chromaprint_get_fingerprint(
        ctx: ChromaprintContext,
        fingerprint: *mut *mut c_char,
    ) -> c_int;
    pub fn chromaprint_get_raw_fingerprint(
        ctx: ChromaprintContext,
        fingerprint: *mut *mut c_void,
        size: *mut c_int,
    ) -> c_int;
    pub fn chromaprint_encode_fingerprint(
        fp: *const c_void,
        size: c_int,
        algorithm: c_int,
        encoded_fp: *mut *mut c_char,
        encoded_size: *mut c_int,
        base64: c_int,
    ) -> c_int;
    pub fn chromaprint_decode_fingerprint(
        encoded_fp: *const c_char,
        encoded_size: c_int,
        fp: *mut *mut c_void,
        size: *mut c_int,
        algorithm: *mut c_int,
        base64: c_int,
    ) -> c_int;
    pub fn chromaprint_get_version() -> *const c_char;
    pub fn chromaprint_set_option(
        ctx: ChromaprintContext,
        name: *const c_char,
        value: c_int,
    ) -> c_int;

    // Moving the free function here so it's in the same unsafe extern block
    fn free(ptr: *mut c_void);
}

// Constants
pub const CHROMAPRINT_ALGORITHM_DEFAULT: c_int = 0;
pub const CHROMAPRINT_ALGORITHM_TEST1: c_int = 1;
pub const CHROMAPRINT_ALGORITHM_TEST2: c_int = 2;
pub const CHROMAPRINT_ALGORITHM_TEST3: c_int = 3;
pub const CHROMAPRINT_ALGORITHM_TEST4: c_int = 4;

// Safe Rust wrapper for Chromaprint
pub struct Chromaprint {
    ctx: ChromaprintContext,
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
            // Use unsafe block around free call
            unsafe { libc_free(fingerprint as *mut c_void) };
            Some(fingerprint_str)
        } else {
            None
        }
    }

    pub fn get_raw_fingerprint(&self) -> Option<Vec<u8>> {
        let mut fingerprint: *mut c_void = std::ptr::null_mut();
        let mut size: c_int = 0;

        let result =
            unsafe { chromaprint_get_raw_fingerprint(self.ctx, &mut fingerprint, &mut size) };

        if result == 1 && !fingerprint.is_null() && size > 0 {
            // Safely convert the raw fingerprint data to a Vec<u8>
            let data = unsafe {
                std::slice::from_raw_parts(fingerprint as *const u8, size as usize).to_vec()
            };

            // Free the memory allocated by chromaprint
            unsafe { libc_free(fingerprint) };

            Some(data)
        } else {
            None
        }
    }

    // You might also want to add convenience methods for encoding/decoding
    pub fn encode_fingerprint(
        raw_fingerprint: &[u8],
        algorithm: c_int,
        base64: bool,
    ) -> Option<String> {
        let mut encoded_fp: *mut c_char = std::ptr::null_mut();
        let mut encoded_size: c_int = 0;

        let result = unsafe {
            chromaprint_encode_fingerprint(
                raw_fingerprint.as_ptr() as *const c_void,
                raw_fingerprint.len() as c_int,
                algorithm,
                &mut encoded_fp,
                &mut encoded_size,
                if base64 { 1 } else { 0 },
            )
        };

        if result == 1 && !encoded_fp.is_null() {
            let c_str = unsafe { std::ffi::CStr::from_ptr(encoded_fp) };
            let encoded_str = c_str.to_string_lossy().into_owned();

            // Free the memory allocated by chromaprint
            unsafe { libc_free(encoded_fp as *mut c_void) };

            Some(encoded_str)
        } else {
            None
        }
    }

    pub fn decode_fingerprint(encoded_fp: &str, base64: bool) -> Option<(Vec<u8>, c_int)> {
        let mut fingerprint: *mut c_void = std::ptr::null_mut();
        let mut size: c_int = 0;
        let mut algorithm: c_int = 0;

        let encoded_bytes = encoded_fp.as_bytes();

        let result = unsafe {
            chromaprint_decode_fingerprint(
                encoded_bytes.as_ptr() as *const c_char,
                encoded_bytes.len() as c_int,
                &mut fingerprint,
                &mut size,
                &mut algorithm,
                if base64 { 1 } else { 0 },
            )
        };

        if result == 1 && !fingerprint.is_null() && size > 0 {
            // Safely convert the raw fingerprint data to a Vec<u8>
            let data = unsafe {
                std::slice::from_raw_parts(fingerprint as *const u8, size as usize).to_vec()
            };

            // Free the memory allocated by chromaprint
            unsafe { libc_free(fingerprint) };

            Some((data, algorithm))
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

// Wrapper to make it clear we're using libc free
unsafe fn libc_free(ptr: *mut c_void) {
    // In Rust 2024, unsafe operations inside unsafe functions
    // still need to be wrapped in an unsafe block
    unsafe { free(ptr) };
}
