#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(improper_ctypes)]

// Include the auto-generated bindings from the build process
include!(concat!(env!("OUT_DIR"), "/chromaprint_bindings.rs"));

use std::ffi::{c_char, c_int, c_void};

// Type alias for compatibility with existing code
pub type ChromaprintContextCompat = *mut ChromaprintContext;

// Re-export algorithm constants for compatibility
pub const CHROMAPRINT_ALGORITHM_DEFAULT: c_int = 1; // TEST2 is the default
pub const CHROMAPRINT_ALGORITHM_TEST1: c_int = 0;
pub const CHROMAPRINT_ALGORITHM_TEST2: c_int = 1;
pub const CHROMAPRINT_ALGORITHM_TEST3: c_int = 2;
pub const CHROMAPRINT_ALGORITHM_TEST4: c_int = 3;
pub const CHROMAPRINT_ALGORITHM_TEST5: c_int = 4;

// Add missing function we need
unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

// Safe Rust wrapper for Chromaprint
pub struct Chromaprint {
    ctx: *mut ChromaprintContext,
}

impl Chromaprint {
    pub fn new(algorithm: c_int) -> Result<Self, &'static str> {
        let ctx = unsafe { chromaprint_new(algorithm) };
        if ctx.is_null() {
            return Err("Failed to create Chromaprint context");
        }
        Ok(Chromaprint { ctx })
    }

    pub fn start(&self, sample_rate: i32, num_channels: i32) -> bool {
        unsafe { chromaprint_start(self.ctx, sample_rate, num_channels) == 1 }
    }

    pub fn feed(&self, data: &[i16]) -> bool {
        unsafe { chromaprint_feed(self.ctx, data.as_ptr() as *const i16, data.len() as c_int) == 1 }
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
            // Free the memory allocated by chromaprint
            unsafe { chromaprint_dealloc(fingerprint as *mut c_void) };
            Some(fingerprint_str)
        } else {
            None
        }
    }

    pub fn get_raw_fingerprint(&self) -> Option<Vec<u32>> {
        let mut fingerprint: *mut u32 = std::ptr::null_mut();
        let mut size: c_int = 0;

        let result =
            unsafe { chromaprint_get_raw_fingerprint(self.ctx, &mut fingerprint, &mut size) };

        if result == 1 && !fingerprint.is_null() && size > 0 {
            // Safely convert the raw fingerprint data to a Vec<u32>
            let data = unsafe {
                std::slice::from_raw_parts(fingerprint, size as usize).to_vec()
            };

            // Free the memory allocated by chromaprint
            unsafe { chromaprint_dealloc(fingerprint as *mut c_void) };

            Some(data)
        } else {
            None
        }
    }

    // Static methods for encoding/decoding
    pub fn encode_fingerprint(
        raw_fingerprint: &[u32],
        algorithm: c_int,
        base64: bool,
    ) -> Option<String> {
        let mut encoded_fp: *mut c_char = std::ptr::null_mut();
        let mut encoded_size: c_int = 0;

        let result = unsafe {
            chromaprint_encode_fingerprint(
                raw_fingerprint.as_ptr(),
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
            unsafe { chromaprint_dealloc(encoded_fp as *mut c_void) };

            Some(encoded_str)
        } else {
            None
        }
    }

    pub fn decode_fingerprint(encoded_fp: &str, base64: bool) -> Option<(Vec<u32>, c_int)> {
        let mut fingerprint: *mut u32 = std::ptr::null_mut();
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
            // Safely convert the raw fingerprint data to a Vec<u32>
            let data = unsafe {
                std::slice::from_raw_parts(fingerprint, size as usize).to_vec()
            };

            // Free the memory allocated by chromaprint
            unsafe { chromaprint_dealloc(fingerprint as *mut c_void) };

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
