

use std::ffi::c_void;
use std::ptr::NonNull;

#[cfg(feature = "dstdec")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Attempted to read beyond the end of the provided DST frame.
    ReadPastEnd,
    /// The frame contains an illegal stuffing or arithmetic pattern.
    InvalidFrame(&'static str),
    /// Output buffer too small (internal error — should not happen if caller
    /// passes `channels * channel_frame_size` bytes).
    OutputTooSmall,
    /// The C++ decoder returned an unexpected error code.
    NativeError(i32),
}

#[cfg(feature = "dstdec")]
#[repr(C)]
struct DstDecoderOpaque {
    _private: [u8; 0],
}
#[cfg(feature = "dstdec")]
unsafe extern "C" {
    fn dst_decoder_new(
        channels: u32,
        channel_frame_size: u32,
    ) -> *mut c_void;

    fn dst_decoder_free(dec: *mut c_void);

    fn dst_decoder_decode(
        dec: *mut c_void,
        dst_data: *const u8,
        dst_data_len: usize,
        out_dsd: *mut u8,
        out_dsd_len: usize,
    ) -> i32;
}


pub struct Decoder {
    ptr: NonNull<c_void>,
    channels: usize,
    channel_frame_size: usize,
}

unsafe impl Send for Decoder {}
unsafe impl Sync for Decoder {}

impl Decoder {#[cfg(feature = "dstdec")]
    /// Create a decoder for `channels` channels and `channel_frame_size`
    /// decoded DSD bytes per channel per frame.
    ///
    /// Panics if the underlying C++ allocation fails (OOM).
    pub fn new(channels: usize, channel_frame_size: usize) -> Self {
        let raw = unsafe {
            dst_decoder_new(channels as u32, channel_frame_size as u32)
        };
        let ptr = NonNull::new(raw)
            .expect("dst_decoder_new returned NULL (OOM or invalid params)");
        Self { ptr, channels, channel_frame_size }
    }
    #[cfg(not(feature = "dstdec"))]
    pub fn new(channels: usize, channel_frame_size: usize) -> Self {
        panic!("This installation does not supports the dst decoding")
    }


    #[cfg(feature = "dstdec")]
    /// Decode a single DST frame.
    ///
    /// - `dst_data`:  raw DSTF chunk payload (compressed bytes).
    /// - `dst_bits`:  `dst_data.len() * 8` — kept for API compatibility with
    ///                the pure-Rust version; the C++ wrapper derives this from
    ///                `dst_data.len()` directly.
    /// - `out_dsd`:   output buffer, must be `channels * channel_frame_size` bytes.
    pub fn decode_frame(
        &mut self,
        dst_data: &[u8],
        _dst_bits: usize,   // ignored — C++ derives from len
        out_dsd: &mut [u8],
    ) -> Result<(), DecodeError> {
        let rv = unsafe {
            dst_decoder_decode(
                self.ptr.as_ptr(),
                dst_data.as_ptr(),
                dst_data.len(),
                out_dsd.as_mut_ptr(),
                out_dsd.len(),
            )
        };
        match rv {
            0  => Ok(()),
            -1 => Err(DecodeError::InvalidFrame("C++ decoder error")),
            -2 => Err(DecodeError::OutputTooSmall),
            n  => Err(DecodeError::NativeError(n)),
        }
    }
    #[cfg(feature = "dstdec")]
    /// Convenience: decode into a freshly allocated `Vec<u8>`.
    pub fn decode_frame_vec(
        &mut self,
        dst_data: &[u8],
        dst_bits: usize,
    ) -> Result<Vec<u8>, DecodeError> {
        let mut out = vec![0u8; self.channels * self.channel_frame_size];
        self.decode_frame(dst_data, dst_bits, &mut out)?;
        Ok(out)
    }
}
#[cfg(feature = "dstdec")]
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { dst_decoder_free(self.ptr.as_ptr()) }
    }
}