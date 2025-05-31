//! WavPack FFI bindings for FFCodex
//!
//! This module provides comprehensive Rust bindings for the WavPack audio compression library.
//! These bindings expose the complete WavPack C API for encoding, decoding, and metadata handling.

use std::os::raw::{c_char, c_int, c_uchar, c_void};

// Type aliases for C integer types
pub type int8_t = i8;
pub type int16_t = i16;
pub type int32_t = i32;
pub type int64_t = i64;
pub type uint8_t = u8;
pub type uint16_t = u16;
pub type uint32_t = u32;
pub type uint64_t = u64;

// ================================== STRUCTURES ==================================

/// RIFF chunk header structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RiffChunkHeader {
    pub ck_id: [c_char; 4],
    pub ck_size: uint32_t,
    pub form_type: [c_char; 4],
}

/// Generic chunk header structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ChunkHeader {
    pub ck_id: [c_char; 4],
    pub ck_size: uint32_t,
}

/// WAV format header structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct WaveHeader {
    pub format_tag: uint16_t,
    pub num_channels: uint16_t,
    pub sample_rate: uint32_t,
    pub bytes_per_second: uint32_t,
    pub block_align: uint16_t,
    pub bits_per_sample: uint16_t,
    pub cb_size: uint16_t,
    pub valid_bits_per_sample: uint16_t,
    pub channel_mask: int32_t,
    pub sub_format: uint16_t,
    pub guid: [c_char; 14],
}

/// WavPack block header structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct WavpackHeader {
    pub ck_id: [c_char; 4],
    pub ck_size: uint32_t,
    pub version: int16_t,
    pub block_index_u8: c_uchar,
    pub total_samples_u8: c_uchar,
    pub total_samples: uint32_t,
    pub block_index: uint32_t,
    pub block_samples: uint32_t,
    pub flags: uint32_t,
    pub crc: uint32_t,
}

/// WavPack configuration structure
#[repr(C)]
#[derive(Debug, Clone)]
pub struct WavpackConfig {
    pub bitrate: f32,
    pub shaping_weight: f32,
    pub bits_per_sample: c_int,
    pub bytes_per_sample: c_int,
    pub qmode: c_int,
    pub flags: c_int,
    pub xmode: c_int,
    pub num_channels: c_int,
    pub float_norm_exp: c_int,
    pub block_samples: int32_t,
    pub worker_threads: int32_t,
    pub sample_rate: int32_t,
    pub channel_mask: int32_t,
    pub md5_checksum: [c_uchar; 16],
    pub md5_read: c_uchar,
    pub num_tag_strings: c_int,
    pub tag_strings: *mut *mut c_char,
}

/// Stream reader callback structure
#[repr(C)]
pub struct WavpackStreamReader {
    pub read_bytes: Option<
        unsafe extern "C" fn(id: *mut c_void, data: *mut c_void, bcount: int32_t) -> int32_t,
    >,
    pub get_pos: Option<unsafe extern "C" fn(id: *mut c_void) -> uint32_t>,
    pub set_pos_abs: Option<unsafe extern "C" fn(id: *mut c_void, pos: uint32_t) -> c_int>,
    pub set_pos_rel:
        Option<unsafe extern "C" fn(id: *mut c_void, delta: int32_t, mode: c_int) -> c_int>,
    pub push_back_byte: Option<unsafe extern "C" fn(id: *mut c_void, c: c_int) -> c_int>,
    pub get_length: Option<unsafe extern "C" fn(id: *mut c_void) -> uint32_t>,
    pub can_seek: Option<unsafe extern "C" fn(id: *mut c_void) -> c_int>,
    pub write_bytes: Option<
        unsafe extern "C" fn(id: *mut c_void, data: *mut c_void, bcount: int32_t) -> int32_t,
    >,
}

/// Extended stream reader for large files (64-bit)
#[repr(C)]
pub struct WavpackStreamReader64 {
    pub read_bytes: Option<
        unsafe extern "C" fn(id: *mut c_void, data: *mut c_void, bcount: int32_t) -> int32_t,
    >,
    pub write_bytes: Option<
        unsafe extern "C" fn(id: *mut c_void, data: *mut c_void, bcount: int32_t) -> int32_t,
    >,
    pub get_pos: Option<unsafe extern "C" fn(id: *mut c_void) -> int64_t>,
    pub set_pos_abs: Option<unsafe extern "C" fn(id: *mut c_void, pos: int64_t) -> c_int>,
    pub set_pos_rel:
        Option<unsafe extern "C" fn(id: *mut c_void, delta: int64_t, mode: c_int) -> c_int>,
    pub push_back_byte: Option<unsafe extern "C" fn(id: *mut c_void, c: c_int) -> c_int>,
    pub get_length: Option<unsafe extern "C" fn(id: *mut c_void) -> int64_t>,
    pub can_seek: Option<unsafe extern "C" fn(id: *mut c_void) -> c_int>,
    pub truncate_here: Option<unsafe extern "C" fn(id: *mut c_void) -> c_int>,
    pub close: Option<unsafe extern "C" fn(id: *mut c_void) -> c_int>,
}

/// Block output callback type
pub type WavpackBlockOutput =
    unsafe extern "C" fn(id: *mut c_void, data: *mut c_void, bcount: int32_t) -> c_int;

/// Opaque WavPack context structure
#[repr(C)]
pub struct WavpackContext {
    _private: [u8; 0],
}

// ================================== CONSTANTS ==================================

// WavPack header flags
pub const BYTES_STORED: uint32_t = 3;
pub const MONO_FLAG: uint32_t = 4;
pub const HYBRID_FLAG: uint32_t = 8;
pub const JOINT_STEREO: uint32_t = 0x10;
pub const CROSS_DECORR: uint32_t = 0x20;
pub const HYBRID_SHAPE: uint32_t = 0x40;
pub const FLOAT_DATA: uint32_t = 0x80;
pub const INT32_DATA: uint32_t = 0x100;
pub const HYBRID_BITRATE: uint32_t = 0x200;
pub const HYBRID_BALANCE: uint32_t = 0x400;
pub const INITIAL_BLOCK: uint32_t = 0x800;
pub const FINAL_BLOCK: uint32_t = 0x1000;

// Shift and mask constants
pub const SHIFT_LSB: uint32_t = 13;
pub const SHIFT_MASK: uint32_t = 0x1f << SHIFT_LSB;
pub const MAG_LSB: uint32_t = 18;
pub const MAG_MASK: uint32_t = 0x1f << MAG_LSB;
pub const SRATE_LSB: uint32_t = 23;
pub const SRATE_MASK: uint32_t = 0xf << SRATE_LSB;

// Additional flags
pub const FALSE_STEREO: uint32_t = 0x40000000;
pub const NEW_SHAPING: uint32_t = 0x20000000;
pub const MONO_DATA: uint32_t = MONO_FLAG | FALSE_STEREO;
pub const HAS_CHECKSUM: uint32_t = 0x10000000;
pub const DSD_FLAG: uint32_t = 0x80000000;
pub const IGNORED_FLAGS: uint32_t = 0x08000000;
pub const UNKNOWN_FLAGS: uint32_t = 0x00000000;

// Version constants
pub const MIN_STREAM_VERS: uint16_t = 0x402;
pub const MAX_STREAM_VERS: uint16_t = 0x410;

// Channel limits
pub const WAVPACK_MAX_CHANS: c_int = 4096;
pub const WAVPACK_MAX_CLI_CHANS: c_int = 256;

// Maximum samples
pub const MAX_WAVPACK_SAMPLES: int64_t = (1i64 << 40) - 257;

// Metadata chunk IDs
pub const ID_UNIQUE: c_uchar = 0x3f;
pub const ID_OPTIONAL_DATA: c_uchar = 0x20;
pub const ID_ODD_SIZE: c_uchar = 0x40;
pub const ID_LARGE: c_uchar = 0x80;

pub const ID_DUMMY: c_uchar = 0x0;
pub const ID_ENCODER_INFO: c_uchar = 0x1;
pub const ID_DECORR_TERMS: c_uchar = 0x2;
pub const ID_DECORR_WEIGHTS: c_uchar = 0x3;
pub const ID_DECORR_SAMPLES: c_uchar = 0x4;
pub const ID_ENTROPY_VARS: c_uchar = 0x5;
pub const ID_HYBRID_PROFILE: c_uchar = 0x6;
pub const ID_SHAPING_WEIGHTS: c_uchar = 0x7;
pub const ID_FLOAT_INFO: c_uchar = 0x8;
pub const ID_INT32_INFO: c_uchar = 0x9;
pub const ID_WV_BITSTREAM: c_uchar = 0xa;
pub const ID_WVC_BITSTREAM: c_uchar = 0xb;
pub const ID_WVX_BITSTREAM: c_uchar = 0xc;
pub const ID_CHANNEL_INFO: c_uchar = 0xd;
pub const ID_DSD_BLOCK: c_uchar = 0xe;

pub const ID_RIFF_HEADER: c_uchar = ID_OPTIONAL_DATA | 0x1;
pub const ID_RIFF_TRAILER: c_uchar = ID_OPTIONAL_DATA | 0x2;
pub const ID_ALT_HEADER: c_uchar = ID_OPTIONAL_DATA | 0x3;
pub const ID_ALT_TRAILER: c_uchar = ID_OPTIONAL_DATA | 0x4;
pub const ID_CONFIG_BLOCK: c_uchar = ID_OPTIONAL_DATA | 0x5;
pub const ID_MD5_CHECKSUM: c_uchar = ID_OPTIONAL_DATA | 0x6;
pub const ID_SAMPLE_RATE: c_uchar = ID_OPTIONAL_DATA | 0x7;
pub const ID_ALT_EXTENSION: c_uchar = ID_OPTIONAL_DATA | 0x8;
pub const ID_ALT_MD5_CHECKSUM: c_uchar = ID_OPTIONAL_DATA | 0x9;
pub const ID_NEW_CONFIG_BLOCK: c_uchar = ID_OPTIONAL_DATA | 0xa;
pub const ID_CHANNEL_IDENTITIES: c_uchar = ID_OPTIONAL_DATA | 0xb;
pub const ID_WVX_NEW_BITSTREAM: c_uchar = ID_OPTIONAL_DATA | ID_WVX_BITSTREAM;
pub const ID_BLOCK_CHECKSUM: c_uchar = ID_OPTIONAL_DATA | 0xf;

// Config flags
pub const CONFIG_HYBRID_FLAG: c_int = 8;
pub const CONFIG_JOINT_STEREO: c_int = 0x10;
pub const CONFIG_CROSS_DECORR: c_int = 0x20;
pub const CONFIG_HYBRID_SHAPE: c_int = 0x40;
pub const CONFIG_FAST_FLAG: c_int = 0x200;
pub const CONFIG_HIGH_FLAG: c_int = 0x800;
pub const CONFIG_VERY_HIGH_FLAG: c_int = 0x1000;
pub const CONFIG_BITRATE_KBPS: c_int = 0x2000;
pub const CONFIG_SHAPE_OVERRIDE: c_int = 0x8000;
pub const CONFIG_JOINT_OVERRIDE: c_int = 0x10000;
pub const CONFIG_DYNAMIC_SHAPING: c_int = 0x20000;
pub const CONFIG_CREATE_EXE: c_int = 0x40000;
pub const CONFIG_CREATE_WVC: c_int = 0x80000;
pub const CONFIG_OPTIMIZE_WVC: c_int = 0x100000;
pub const CONFIG_COMPATIBLE_WRITE: c_int = 0x400000;
pub const CONFIG_CALC_NOISE: c_int = 0x800000;
pub const CONFIG_EXTRA_MODE: c_int = 0x2000000;
pub const CONFIG_SKIP_WVX: c_int = 0x4000000;
pub const CONFIG_MD5_CHECKSUM: c_int = 0x8000000;
pub const CONFIG_MERGE_BLOCKS: c_int = 0x10000000;
pub const CONFIG_PAIR_UNDEF_CHANS: c_int = 0x20000000;
pub const CONFIG_OPTIMIZE_32BIT: c_int = 0x40000000;
pub const CONFIG_OPTIMIZE_MONO: c_int = 0x80000000u32 as c_int;

// QMode flags
pub const QMODE_BIG_ENDIAN: c_int = 0x1;
pub const QMODE_SIGNED_BYTES: c_int = 0x2;
pub const QMODE_UNSIGNED_WORDS: c_int = 0x4;
pub const QMODE_REORDERED_CHANS: c_int = 0x8;
pub const QMODE_DSD_LSB_FIRST: c_int = 0x10;
pub const QMODE_DSD_MSB_FIRST: c_int = 0x20;
pub const QMODE_DSD_IN_BLOCKS: c_int = 0x40;
pub const QMODE_DSD_AUDIO: c_int = QMODE_DSD_LSB_FIRST | QMODE_DSD_MSB_FIRST;

// Command-line specific flags (library ignores these)
pub const QMODE_ADOBE_MODE: c_int = 0x100;
pub const QMODE_NO_STORE_WRAPPER: c_int = 0x200;
pub const QMODE_CHANS_UNASSIGNED: c_int = 0x400;
pub const QMODE_IGNORE_LENGTH: c_int = 0x800;
pub const QMODE_RAW_PCM: c_int = 0x1000;
pub const QMODE_EVEN_BYTE_DEPTH: c_int = 0x2000;

// Open flags
pub const OPEN_WVC: c_int = 0x1;
pub const OPEN_TAGS: c_int = 0x2;
pub const OPEN_WRAPPER: c_int = 0x4;
pub const OPEN_2CH_MAX: c_int = 0x8;
pub const OPEN_NORMALIZE: c_int = 0x10;
pub const OPEN_STREAMING: c_int = 0x20;
pub const OPEN_EDIT_TAGS: c_int = 0x40;
pub const OPEN_FILE_UTF8: c_int = 0x80;
pub const OPEN_DSD_NATIVE: c_int = 0x100;
pub const OPEN_DSD_AS_PCM: c_int = 0x200;
pub const OPEN_ALT_TYPES: c_int = 0x400;
pub const OPEN_NO_CHECKSUM: c_int = 0x800;

// Thread flags
pub const OPEN_THREADS_SHFT: c_int = 12;
pub const OPEN_THREADS_MASK: c_int = 0xF000;

// Mode flags
pub const MODE_WVC: c_int = 0x1;
pub const MODE_LOSSLESS: c_int = 0x2;
pub const MODE_HYBRID: c_int = 0x4;
pub const MODE_FLOAT: c_int = 0x8;
pub const MODE_VALID_TAG: c_int = 0x10;
pub const MODE_HIGH: c_int = 0x20;
pub const MODE_FAST: c_int = 0x40;
pub const MODE_EXTRA: c_int = 0x80;
pub const MODE_APETAG: c_int = 0x100;
pub const MODE_SFX: c_int = 0x200;
pub const MODE_VERY_HIGH: c_int = 0x400;
pub const MODE_MD5: c_int = 0x800;
pub const MODE_XMODE: c_int = 0x7000;
pub const MODE_DNS: c_int = 0x8000;

// File format types
pub const WP_FORMAT_WAV: c_uchar = 0;
pub const WP_FORMAT_W64: c_uchar = 1;
pub const WP_FORMAT_CAF: c_uchar = 2;
pub const WP_FORMAT_DFF: c_uchar = 3;
pub const WP_FORMAT_DSF: c_uchar = 4;
pub const WP_FORMAT_AIF: c_uchar = 5;

// ================================== FUNCTIONS ==================================

#[link(name = "wavpack")]
unsafe extern "C" {
    // ======================== Decoding Functions ========================

    /// Open a WavPack file for decoding from a filename
    pub fn WavpackOpenFileInput(
        infilename: *const c_char,
        error: *mut c_char,
        flags: c_int,
        norm_offset: c_int,
    ) -> *mut WavpackContext;

    /// Open a WavPack stream for decoding with custom reader
    pub fn WavpackOpenFileInputEx(
        reader: *mut WavpackStreamReader,
        wv_id: *mut c_void,
        wvc_id: *mut c_void,
        error: *mut c_char,
        flags: c_int,
        norm_offset: c_int,
    ) -> *mut WavpackContext;

    /// Open a WavPack stream for decoding with 64-bit reader
    pub fn WavpackOpenFileInputEx64(
        reader: *mut WavpackStreamReader64,
        wv_id: *mut c_void,
        wvc_id: *mut c_void,
        error: *mut c_char,
        flags: c_int,
        norm_offset: c_int,
    ) -> *mut WavpackContext;

    /// Open raw WavPack data for decoding
    pub fn WavpackOpenRawDecoder(
        main_data: *mut c_void,
        main_size: int32_t,
        corr_data: *mut c_void,
        corr_size: int32_t,
        version: int16_t,
        error: *mut c_char,
        flags: c_int,
        norm_offset: c_int,
    ) -> *mut WavpackContext;

    /// Unpack audio samples from WavPack stream
    pub fn WavpackUnpackSamples(
        wpc: *mut WavpackContext,
        buffer: *mut int32_t,
        samples: uint32_t,
    ) -> uint32_t;

    /// Seek to a specific sample position
    pub fn WavpackSeekSample(wpc: *mut WavpackContext, sample: uint32_t) -> c_int;

    /// Seek to a specific sample position (64-bit)
    pub fn WavpackSeekSample64(wpc: *mut WavpackContext, sample: int64_t) -> c_int;

    /// Close WavPack context and free resources
    pub fn WavpackCloseFile(wpc: *mut WavpackContext) -> *mut WavpackContext;

    // ======================== Information Functions ========================

    /// Get the mode flags for the WavPack stream
    pub fn WavpackGetMode(wpc: *mut WavpackContext) -> c_int;

    /// Get the qualify mode flags
    pub fn WavpackGetQualifyMode(wpc: *mut WavpackContext) -> c_int;

    /// Get error message string
    pub fn WavpackGetErrorMessage(wpc: *mut WavpackContext) -> *mut c_char;

    /// Get WavPack version
    pub fn WavpackGetVersion(wpc: *mut WavpackContext) -> c_int;

    /// Get file extension
    pub fn WavpackGetFileExtension(wpc: *mut WavpackContext) -> *mut c_char;

    /// Get file format
    pub fn WavpackGetFileFormat(wpc: *mut WavpackContext) -> c_uchar;

    /// Get total number of samples
    pub fn WavpackGetNumSamples(wpc: *mut WavpackContext) -> uint32_t;

    /// Get total number of samples (64-bit)
    pub fn WavpackGetNumSamples64(wpc: *mut WavpackContext) -> int64_t;

    /// Get number of samples in current frame
    pub fn WavpackGetNumSamplesInFrame(wpc: *mut WavpackContext) -> uint32_t;

    /// Get current sample index
    pub fn WavpackGetSampleIndex(wpc: *mut WavpackContext) -> uint32_t;

    /// Get current sample index (64-bit)
    pub fn WavpackGetSampleIndex64(wpc: *mut WavpackContext) -> int64_t;

    /// Get number of errors encountered
    pub fn WavpackGetNumErrors(wpc: *mut WavpackContext) -> c_int;

    /// Check if any blocks were lossy
    pub fn WavpackLossyBlocks(wpc: *mut WavpackContext) -> c_int;

    /// Get sample rate
    pub fn WavpackGetSampleRate(wpc: *mut WavpackContext) -> uint32_t;

    /// Get native sample rate
    pub fn WavpackGetNativeSampleRate(wpc: *mut WavpackContext) -> uint32_t;

    /// Get bits per sample
    pub fn WavpackGetBitsPerSample(wpc: *mut WavpackContext) -> c_int;

    /// Get bytes per sample
    pub fn WavpackGetBytesPerSample(wpc: *mut WavpackContext) -> c_int;

    /// Get number of channels
    pub fn WavpackGetNumChannels(wpc: *mut WavpackContext) -> c_int;

    /// Get channel mask
    pub fn WavpackGetChannelMask(wpc: *mut WavpackContext) -> c_int;

    /// Get reduced number of channels
    pub fn WavpackGetReducedChannels(wpc: *mut WavpackContext) -> c_int;

    /// Get float normalization exponent
    pub fn WavpackGetFloatNormExp(wpc: *mut WavpackContext) -> c_int;

    /// Get MD5 checksum
    pub fn WavpackGetMD5Sum(wpc: *mut WavpackContext, data: *mut c_uchar) -> c_int;

    /// Get channel identities
    pub fn WavpackGetChannelIdentities(wpc: *mut WavpackContext, identities: *mut c_uchar);

    /// Get channel layout
    pub fn WavpackGetChannelLayout(wpc: *mut WavpackContext, reorder: *mut c_uchar) -> uint32_t;

    /// Get wrapper bytes count
    pub fn WavpackGetWrapperBytes(wpc: *mut WavpackContext) -> uint32_t;

    /// Get wrapper data
    pub fn WavpackGetWrapperData(wpc: *mut WavpackContext) -> *mut c_uchar;

    /// Free wrapper data
    pub fn WavpackFreeWrapper(wpc: *mut WavpackContext);

    /// Seek to trailing wrapper
    pub fn WavpackSeekTrailingWrapper(wpc: *mut WavpackContext);

    /// Get decoding progress
    pub fn WavpackGetProgress(wpc: *mut WavpackContext) -> f64;

    /// Get file size
    pub fn WavpackGetFileSize(wpc: *mut WavpackContext) -> uint32_t;

    /// Get file size (64-bit)
    pub fn WavpackGetFileSize64(wpc: *mut WavpackContext) -> int64_t;

    /// Get compression ratio
    pub fn WavpackGetRatio(wpc: *mut WavpackContext) -> f64;

    /// Get average bitrate
    pub fn WavpackGetAverageBitrate(wpc: *mut WavpackContext, count_wvc: c_int) -> f64;

    /// Get instantaneous bitrate
    pub fn WavpackGetInstantBitrate(wpc: *mut WavpackContext) -> f64;

    // ======================== Metadata Functions ========================

    /// Get number of tag items
    pub fn WavpackGetNumTagItems(wpc: *mut WavpackContext) -> c_int;

    /// Get a tag item by name
    pub fn WavpackGetTagItem(
        wpc: *mut WavpackContext,
        item: *const c_char,
        value: *mut c_char,
        size: c_int,
    ) -> c_int;

    /// Get a tag item by index
    pub fn WavpackGetTagItemIndexed(
        wpc: *mut WavpackContext,
        index: c_int,
        item: *mut c_char,
        size: c_int,
    ) -> c_int;

    /// Get number of binary tag items
    pub fn WavpackGetNumBinaryTagItems(wpc: *mut WavpackContext) -> c_int;

    /// Get a binary tag item by name
    pub fn WavpackGetBinaryTagItem(
        wpc: *mut WavpackContext,
        item: *const c_char,
        value: *mut c_char,
        size: c_int,
    ) -> c_int;

    /// Get a binary tag item by index
    pub fn WavpackGetBinaryTagItemIndexed(
        wpc: *mut WavpackContext,
        index: c_int,
        item: *mut c_char,
        size: c_int,
    ) -> c_int;

    /// Append a tag item
    pub fn WavpackAppendTagItem(
        wpc: *mut WavpackContext,
        item: *const c_char,
        value: *const c_char,
        vsize: c_int,
    ) -> c_int;

    /// Append a binary tag item
    pub fn WavpackAppendBinaryTagItem(
        wpc: *mut WavpackContext,
        item: *const c_char,
        value: *const c_char,
        vsize: c_int,
    ) -> c_int;

    /// Delete a tag item
    pub fn WavpackDeleteTagItem(wpc: *mut WavpackContext, item: *const c_char) -> c_int;

    /// Write tags to file
    pub fn WavpackWriteTag(wpc: *mut WavpackContext) -> c_int;

    // ======================== Encoding Functions ========================

    /// Open WavPack file for output
    pub fn WavpackOpenFileOutput(
        blockout: WavpackBlockOutput,
        wv_id: *mut c_void,
        wvc_id: *mut c_void,
    ) -> *mut WavpackContext;

    /// Set file information for encoding
    pub fn WavpackSetFileInformation(
        wpc: *mut WavpackContext,
        file_extension: *mut c_char,
        file_format: c_uchar,
    );

    /// Set encoding configuration
    pub fn WavpackSetConfiguration(
        wpc: *mut WavpackContext,
        config: *mut WavpackConfig,
        total_samples: uint32_t,
    ) -> c_int;

    /// Set encoding configuration (64-bit)
    pub fn WavpackSetConfiguration64(
        wpc: *mut WavpackContext,
        config: *mut WavpackConfig,
        total_samples: int64_t,
        chan_ids: *const c_uchar,
    ) -> c_int;

    /// Set channel layout
    pub fn WavpackSetChannelLayout(
        wpc: *mut WavpackContext,
        layout_tag: uint32_t,
        reorder: *const c_uchar,
    ) -> c_int;

    /// Add wrapper data
    pub fn WavpackAddWrapper(
        wpc: *mut WavpackContext,
        data: *mut c_void,
        bcount: uint32_t,
    ) -> c_int;

    /// Store MD5 checksum
    pub fn WavpackStoreMD5Sum(wpc: *mut WavpackContext, data: *mut c_uchar) -> c_int;

    /// Initialize packing
    pub fn WavpackPackInit(wpc: *mut WavpackContext) -> c_int;

    /// Pack audio samples
    pub fn WavpackPackSamples(
        wpc: *mut WavpackContext,
        sample_buffer: *mut int32_t,
        sample_count: uint32_t,
    ) -> c_int;

    /// Flush remaining samples
    pub fn WavpackFlushSamples(wpc: *mut WavpackContext) -> c_int;

    /// Update number of samples
    pub fn WavpackUpdateNumSamples(wpc: *mut WavpackContext, first_block: *mut c_void);

    /// Get wrapper location
    pub fn WavpackGetWrapperLocation(first_block: *mut c_void, size: *mut uint32_t) -> *mut c_void;

    /// Get encoded noise
    pub fn WavpackGetEncodedNoise(wpc: *mut WavpackContext, peak: *mut f64) -> f64;

    // ======================== Utility Functions ========================

    /// Verify a single WavPack block
    pub fn WavpackVerifySingleBlock(buffer: *mut c_uchar, verify_checksum: c_int) -> c_int;

    /// Normalize float values
    pub fn WavpackFloatNormalize(values: *mut int32_t, num_values: int32_t, delta_exp: c_int);

    /// Convert little endian to native
    pub fn WavpackLittleEndianToNative(data: *mut c_void, format: *mut c_char);

    /// Convert native to little endian
    pub fn WavpackNativeToLittleEndian(data: *mut c_void, format: *mut c_char);

    /// Convert big endian to native
    pub fn WavpackBigEndianToNative(data: *mut c_void, format: *mut c_char);

    /// Convert native to big endian
    pub fn WavpackNativeToBigEndian(data: *mut c_void, format: *mut c_char);

    /// Get library version number
    pub fn WavpackGetLibraryVersion() -> uint32_t;

    /// Get library version string
    pub fn WavpackGetLibraryVersionString() -> *const c_char;
}

// ================================== HELPER MACROS ==================================

/// Get block index from header (40-bit field)
pub fn get_block_index(hdr: &WavpackHeader) -> int64_t {
    hdr.block_index as int64_t + ((hdr.block_index_u8 as int64_t) << 32)
}

/// Set block index in header (40-bit field)
pub fn set_block_index(hdr: &mut WavpackHeader, value: int64_t) {
    // Take the bottom 32 bits for block_index and bits 40-47 for block_index_u8
    hdr.block_index = (value & 0xFFFFFFFF) as uint32_t;
    hdr.block_index_u8 = ((value >> 40) & 0xFF) as c_uchar;
}

/// Get total samples from header (40-bit field with special handling for unknown)
pub fn get_total_samples(hdr: &WavpackHeader) -> int64_t {
    if hdr.total_samples == u32::MAX {
        -1
    } else {
        hdr.total_samples as int64_t + ((hdr.total_samples_u8 as int64_t) << 32)
            - hdr.total_samples_u8 as int64_t
    }
}

/// Set total samples in header (40-bit field with special handling for unknown)
pub fn set_total_samples(hdr: &mut WavpackHeader, value: int64_t) {
    if value < 0 {
        hdr.total_samples = u32::MAX;
    } else {
        let tmp = value + (value / 0xffffffffi64);
        hdr.total_samples = tmp as uint32_t;
        hdr.total_samples_u8 = (tmp >> 32) as c_uchar;
    }
}

// ================================== SAFETY WRAPPERS ==================================

impl Default for WavpackConfig {
    fn default() -> Self {
        Self {
            bitrate: 0.0,
            shaping_weight: 0.0,
            bits_per_sample: 0,
            bytes_per_sample: 0,
            qmode: 0,
            flags: 0,
            xmode: 0,
            num_channels: 0,
            float_norm_exp: 0,
            block_samples: 0,
            worker_threads: 0,
            sample_rate: 0,
            channel_mask: 0,
            md5_checksum: [0; 16],
            md5_read: 0,
            num_tag_strings: 0,
            tag_strings: std::ptr::null_mut(),
        }
    }
}

impl Default for WavpackStreamReader {
    fn default() -> Self {
        Self {
            read_bytes: None,
            get_pos: None,
            set_pos_abs: None,
            set_pos_rel: None,
            push_back_byte: None,
            get_length: None,
            can_seek: None,
            write_bytes: None,
        }
    }
}

impl Default for WavpackStreamReader64 {
    fn default() -> Self {
        Self {
            read_bytes: None,
            write_bytes: None,
            get_pos: None,
            set_pos_abs: None,
            set_pos_rel: None,
            push_back_byte: None,
            get_length: None,
            can_seek: None,
            truncate_here: None,
            close: None,
        }
    }
}

// ================================== TESTS ==================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structure_sizes() {
        // Basic sanity checks for structure sizes
        assert!(std::mem::size_of::<WavpackConfig>() > 0);
        assert!(std::mem::size_of::<WavpackHeader>() > 0);
        assert!(std::mem::size_of::<WavpackStreamReader>() > 0);
        assert!(std::mem::size_of::<WavpackStreamReader64>() > 0);
    }

    #[test]
    fn test_constants() {
        // Verify some key constants
        assert_eq!(WAVPACK_MAX_CHANS, 4096);
        assert_eq!(MIN_STREAM_VERS, 0x402);
        assert_eq!(MAX_STREAM_VERS, 0x410);
    }

    #[test]
    fn test_block_index_macros() {
        let mut header = WavpackHeader {
            ck_id: [0; 4],
            ck_size: 0,
            version: 0,
            block_index_u8: 0x12,
            total_samples_u8: 0,
            total_samples: 0,
            block_index: 0x34567890,
            block_samples: 0,
            flags: 0,
            crc: 0,
        };

        let index = get_block_index(&header);
        assert_eq!(index, 0x1234567890);

        set_block_index(&mut header, 0xABCDEF123456);

        assert_eq!(header.block_index, 0xEF123456);
        assert_eq!(header.block_index_u8, 0xAB);
    }
}
