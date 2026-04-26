/*
* Direct Stream Transfer (DST) codec
* ISO/IEC 14496-3 Part 3 Subpart 10: Technical description of lossless coding of oversampled audio
*/

#ifndef CONSTS_H
#define CONSTS_H


// Prediction
constexpr int DST_SIZE_CODEDPREDORDER = 7; // Number of bits in the stream for representing	the CodedPredOrder in each frame
constexpr int DST_MAXPREDORDER = 1 << DST_SIZE_CODEDPREDORDER; // Maximum prediction filter order

// Probability tables
constexpr int DST_SIZE_CODEDPTABLELEN = 6; // Number bits for p-table length
constexpr int DST_MAXPTABLELEN = 1 << DST_SIZE_CODEDPTABLELEN; // Maximum length of p-tables

constexpr int DST_SIZE_PREDCOEF = 9; // Number of bits in the stream for representing each filter coefficient in each frame

// Arithmetic coding
constexpr int DST_AC_BITS    = 8; // Number of bits and maximum level for coding the probability
constexpr int DST_AC_PROBS   = 1 << DST_AC_BITS;
constexpr int DST_AC_HISBITS = 6; // Number of entries in the histogram
constexpr int DST_AC_HISMAX  = 1 << DST_AC_HISBITS;
constexpr int DST_AC_QSTEP   = DST_SIZE_PREDCOEF - DST_AC_HISBITS; // Quantization step for histogram

// Rice coding of filter coefficients and probability tables
constexpr int DST_NROFFRICEMETHODS = 3; // Number of different Pred. Methods for filters	used in combination with Rice coding
constexpr int DST_NROFPRICEMETHODS = 3; // Number of different Pred. Methods for Ptables	used in combination with Rice coding
constexpr int DST_MAXCPREDORDER    = 3; // max pred.order for prediction of filter coefs / Ptables entries
constexpr int DST_SIZE_RICEMETHOD  = 2; // nr of bits in stream for indicating method
constexpr int DST_SIZE_RICEM       = 3; // nr of bits in stream for indicating m
constexpr int DST_MAX_RICE_M_F     = 6; // Max. value of m for filters
constexpr int DST_MAX_RICE_M_P     = 4; // Max. value of m for Ptables

// Segmentation
constexpr int DST_MAXNROF_FSEGS = 4;   // max nr of segments per channel for filters
constexpr int DST_MAXNROF_PSEGS = 8;   // max nr of segments per channel for Ptables
constexpr int DST_MIN_FSEG_LEN = 1024; // min segment length in bits of filters
constexpr int DST_MIN_PSEG_LEN = 32;   // min segment length in bits of Ptables

constexpr int DST_MAXNROF_SEGS = 8; // max nr of segments per channel for filters or Ptables


#endif
