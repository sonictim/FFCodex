// src/wavpack.rs
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// Include the generated bindings
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// Add safe Rust wrappers around the raw C API
use std::ffi::{CStr, CString};
use std::ptr;

pub struct WavpackContext(*mut WavpackContext);

impl WavpackContext {
    pub fn open_file(filename: &str) -> Result<Self, String> {
        let c_filename = CString::new(filename).unwrap();
        let mut error = [0i8; 80];

        let ctx = unsafe { WavpackOpenFileInput(c_filename.as_ptr(), error.as_mut_ptr(), 0, 0) };

        if ctx.is_null() {
            let error_str = unsafe { CStr::from_ptr(error.as_ptr()) };
            Err(error_str.to_string_lossy().into_owned())
        } else {
            Ok(WavpackContext(ctx))
        }
    }

    pub fn get_num_channels(&self) -> i32 {
        unsafe { WavpackGetNumChannels(self.0) }
    }

    pub fn get_sample_rate(&self) -> u32 {
        unsafe { WavpackGetSampleRate(self.0) }
    }

    // Add more wrapper methods as needed
}

impl Drop for WavpackContext {
    fn drop(&mut self) {
        unsafe {
            WavpackCloseFile(self.0);
        }
    }
}
