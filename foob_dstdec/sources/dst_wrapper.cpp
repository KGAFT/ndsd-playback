/*
* Thin C wrapper around the C++ DST decoder so Rust can call it via FFI.
 * Place this file alongside the existing C++ decoder sources.
 */
#include <iostream>
extern void log_printf(const char* text, ...) {
    std::cout << text << std::endl;
}

#include "dst_wrapper.h"
#include "decoder/decoder.h"   // dst::decoder_t

#include <cstring>
#include <new>

// The C++ decoder_t needs to know channels and channel_frame_size at
// construction time (via init()), so we bundle both together.
struct DstDecoder {
    dst::decoder_t  dec;
    unsigned int    channels;
    unsigned int    channel_frame_size;
};

extern "C" {

void* dst_decoder_new(uint32_t channels, uint32_t channel_frame_size) {
    DstDecoder* h = new (std::nothrow) DstDecoder();
    if (!h) return nullptr;
    h->channels           = channels;
    h->channel_frame_size = channel_frame_size;
    if (h->dec.init(channels, channel_frame_size) != 0) {
        delete h;
        return nullptr;
    }
    return h;
}

void dst_decoder_free(void* d) {
    auto* dec = (DstDecoder*)d;
    if (dec) {
        dec->dec.close();
        delete dec;
    }
}

int dst_decoder_decode(void* d,
                       const uint8_t* dst_data,
                       size_t         dst_data_len,
                       uint8_t*       out_dsd,
                       size_t         out_dsd_len)
{

    auto* dec = (DstDecoder*)d;
    if (!dec || !dst_data || !out_dsd) return -1;

    const size_t required = (size_t)dec->channels * dec->channel_frame_size;
    if (out_dsd_len < required) return -2;

    // Zero output before decoding (decoder ORs bits in for compressed frames)
    std::memset(out_dsd, 0, required);

    // dst_bits = compressed frame size in bits, exactly as the C++ decoder expects
    const unsigned int dst_bits = (unsigned int)(dst_data_len * 8);

    int rv = dec->dec.run(dst_data, dst_bits, out_dsd);
    return rv;  // 0 = ok, -1 = error
}

} // extern "C"