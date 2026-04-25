#pragma once
#include <stdint.h>
#include <stddef.h>




extern "C" {

/// Opaque handle to a DST decoder instance.
typedef struct DstDecoder DstDecoder;

/// Create a new decoder for `channels` channels and `channel_frame_size`
/// decoded bytes per channel per frame.
/// Returns NULL on allocation failure.
void* dst_decoder_new(uint32_t channels, uint32_t channel_frame_size);

/// Free a decoder created by dst_decoder_new.
void dst_decoder_free(void* dec);

/// Decode one DST frame.
///
/// @param dec              decoder handle
/// @param dst_data         compressed DSTF payload bytes
/// @param dst_data_len     byte length of dst_data (= DSTF chunk_size)
/// @param out_dsd          output buffer, must be channels * channel_frame_size bytes
/// @param out_dsd_len      byte length of out_dsd (for bounds checking)
/// @return  0 on success
///         -1 on decode error
///         -2 if out_dsd_len is too small
int dst_decoder_decode(void*    dec,
                       const uint8_t* dst_data,
                       size_t         dst_data_len,
                       uint8_t*       out_dsd,
                       size_t         out_dsd_len);


}
