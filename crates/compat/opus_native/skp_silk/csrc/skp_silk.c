#include <string.h>
#include "opus_types.h"
#include "API.h"
#include "control.h"
#include "define.h"
#include "entenc.h"
#include "entdec.h"

#define SKP_API __declspec(dllexport)

#define SKP_SILK_NO_ERROR 0
#define SKP_SILK_ENC_INPUT_INVALID_NO_OF_SAMPLES -1
#define SKP_SILK_ENC_FS_NOT_SUPPORTED -2
#define SKP_SILK_ENC_PACKET_SIZE_NOT_SUPPORTED -3
#define SKP_SILK_ENC_PAYLOAD_BUF_TOO_SHORT -4
#define SKP_SILK_ENC_INVALID_LOSS_RATE -5
#define SKP_SILK_ENC_INVALID_COMPLEXITY_SETTING -6
#define SKP_SILK_ENC_INVALID_INBAND_FEC_SETTING -7
#define SKP_SILK_ENC_INVALID_DTX_SETTING -8
#define SKP_SILK_ENC_INTERNAL_ERROR -9
#define SKP_SILK_DEC_INVALID_SAMPLING_FREQUENCY -10
#define SKP_SILK_DEC_PAYLOAD_TOO_LARGE -11
#define SKP_SILK_DEC_PAYLOAD_ERROR -12

#define ENC_MAGIC 0x534b5045
#define DEC_MAGIC 0x534b5044
#define MAX_RATE 48000
#define MAX_PACKET_MS 100
#define MAX_BUF_SAMPLES (2 * MAX_RATE * MAX_PACKET_MS / 1000)
#define MAX_PACKET_BYTES 4096
#define CHUNK_HDR 4
#define MAX_CHUNKS 2
#define STATE_ALIGN 16

typedef struct {
    opus_int32 API_sampleRate;
    opus_int32 maxInternalSampleRate;
    opus_int32 packetSize;
    opus_int32 bitRate;
    opus_int32 packetLossPercentage;
    opus_int32 complexity;
    opus_int32 useInBandFEC;
    opus_int32 useDTX;
} SKP_SILK_SDK_EncControlStruct;

typedef struct {
    opus_int32 API_sampleRate;
    opus_int32 frameSize;
    opus_int32 framesPerPacket;
    opus_int32 moreInternalDecoderFrames;
    opus_int32 inBandFECOffset;
} SKP_SILK_SDK_DecControlStruct;

typedef struct {
    opus_int32 magic;
    opus_int32 api_rate;
    opus_int32 buffered;
    SKP_SILK_SDK_EncControlStruct last;
    silk_EncControlStruct silk;
    opus_int16 buf[MAX_BUF_SAMPLES];
} EncHeader;

typedef struct {
    opus_int32 magic;
    opus_int32 api_rate;
    opus_int32 internal_rate;
    opus_int32 pkt_len;
    opus_int32 chunk_count;
    opus_int32 chunk;
    opus_int32 chunk_frames_left;
    opus_int32 chunk_started;
    opus_int32 frames_total;
    opus_int32 frames_left;
    opus_int32 chunk_off[MAX_CHUNKS];
    opus_int32 chunk_len[MAX_CHUNKS];
    opus_int32 chunk_ms[MAX_CHUNKS];
    opus_int32 chunk_rate[MAX_CHUNKS];
    ec_dec dec;
    silk_DecControlStruct silk;
    unsigned char pkt[MAX_PACKET_BYTES];
} DecHeader;

static int header_size(int n) {
    return (n + STATE_ALIGN - 1) / STATE_ALIGN * STATE_ALIGN;
}

static void *enc_silk(void *st) {
    return (char *)st + header_size(sizeof(EncHeader));
}

static void *dec_silk(void *st) {
    return (char *)st + header_size(sizeof(DecHeader));
}

static int rate_ok(opus_int32 r) {
    return r == 8000 || r == 12000 || r == 16000 || r == 24000 || r == 32000 || r == 44100 || r == 48000;
}

static int split_packet(int ms, int *parts) {
    switch (ms) {
    case 20: parts[0] = 20; return 1;
    case 40: parts[0] = 40; return 1;
    case 60: parts[0] = 60; return 1;
    case 80: parts[0] = 40; parts[1] = 40; return 2;
    case 100: parts[0] = 40; parts[1] = 60; return 2;
    default: return 0;
    }
}

static opus_int32 internal_cap(opus_int32 skp_max) {
    if (skp_max <= 8000) return 8000;
    if (skp_max <= 12000) return 12000;
    return 16000;
}

static int silk_complexity(opus_int32 c) {
    return c == 0 ? 2 : c == 1 ? 6 : 10;
}

SKP_API int SKP_Silk_SDK_Get_Encoder_Size(opus_int32 *encSizeBytes) {
    opus_int n = 0;
    if (!encSizeBytes) return SKP_SILK_ENC_INTERNAL_ERROR;
    if (silk_Get_Encoder_Size(&n, 1)) return SKP_SILK_ENC_INTERNAL_ERROR;
    *encSizeBytes = header_size(sizeof(EncHeader)) + n;
    return SKP_SILK_NO_ERROR;
}

static void report(const EncHeader *h, SKP_SILK_SDK_EncControlStruct *out) {
    out->API_sampleRate = h->silk.API_sampleRate;
    out->maxInternalSampleRate = h->silk.maxInternalSampleRate;
    out->packetSize = h->silk.API_sampleRate * h->silk.payloadSize_ms / 1000;
    out->bitRate = h->silk.bitRate;
    out->packetLossPercentage = h->silk.packetLossPercentage;
    out->complexity = h->silk.complexity <= 2 ? 0 : h->silk.complexity <= 6 ? 1 : 2;
    out->useInBandFEC = h->silk.useInBandFEC;
    out->useDTX = h->silk.useDTX;
}

SKP_API int SKP_Silk_SDK_InitEncoder(void *encState, SKP_SILK_SDK_EncControlStruct *encStatus) {
    EncHeader *h = (EncHeader *)encState;
    if (!encState) return SKP_SILK_ENC_INTERNAL_ERROR;
    memset(h, 0, sizeof(EncHeader));
    if (silk_InitEncoder(enc_silk(encState), 1, 0, &h->silk)) return SKP_SILK_ENC_INTERNAL_ERROR;
    h->magic = ENC_MAGIC;
    h->api_rate = h->silk.API_sampleRate;
    if (encStatus) report(h, encStatus);
    return SKP_SILK_NO_ERROR;
}

SKP_API int SKP_Silk_SDK_QueryEncoder(const void *encState, SKP_SILK_SDK_EncControlStruct *encStatus) {
    const EncHeader *h = (const EncHeader *)encState;
    if (!h || h->magic != ENC_MAGIC || !encStatus) return SKP_SILK_ENC_INTERNAL_ERROR;
    report(h, encStatus);
    return SKP_SILK_NO_ERROR;
}

static int validate(const SKP_SILK_SDK_EncControlStruct *c, int *packet_ms) {
    if (!rate_ok(c->API_sampleRate)) return SKP_SILK_ENC_FS_NOT_SUPPORTED;
    if (c->maxInternalSampleRate != 8000 && c->maxInternalSampleRate != 12000 && c->maxInternalSampleRate != 16000 && c->maxInternalSampleRate != 24000) return SKP_SILK_ENC_FS_NOT_SUPPORTED;
    if (c->packetSize <= 0 || (c->packetSize * 1000) % c->API_sampleRate) return SKP_SILK_ENC_PACKET_SIZE_NOT_SUPPORTED;
    *packet_ms = c->packetSize * 1000 / c->API_sampleRate;
    {
        int parts[MAX_CHUNKS];
        if (!split_packet(*packet_ms, parts)) return SKP_SILK_ENC_PACKET_SIZE_NOT_SUPPORTED;
    }
    if (c->packetLossPercentage < 0 || c->packetLossPercentage > 100) return SKP_SILK_ENC_INVALID_LOSS_RATE;
    if (c->complexity < 0 || c->complexity > 2) return SKP_SILK_ENC_INVALID_COMPLEXITY_SETTING;
    if (c->useInBandFEC < 0 || c->useInBandFEC > 1) return SKP_SILK_ENC_INVALID_INBAND_FEC_SETTING;
    if (c->useDTX < 0 || c->useDTX > 1) return SKP_SILK_ENC_INVALID_DTX_SETTING;
    return SKP_SILK_NO_ERROR;
}

static int encode_chunk(EncHeader *h, void *silk_state, const SKP_SILK_SDK_EncControlStruct *c, int ms, const opus_int16 *pcm, unsigned char *out, int cap) {
    ec_enc enc;
    opus_int32 nbytes;
    int samples = c->API_sampleRate * ms / 1000;
    int body = cap - CHUNK_HDR;
    int used;
    if (body < 1) return SKP_SILK_ENC_PAYLOAD_BUF_TOO_SHORT;
    if (body > 1275) body = 1275;
    h->silk.nChannelsAPI = 1;
    h->silk.nChannelsInternal = 1;
    h->silk.API_sampleRate = c->API_sampleRate;
    h->silk.maxInternalSampleRate = internal_cap(c->maxInternalSampleRate);
    h->silk.minInternalSampleRate = 8000;
    h->silk.desiredInternalSampleRate = h->silk.maxInternalSampleRate;
    h->silk.payloadSize_ms = ms;
    h->silk.bitRate = c->bitRate;
    h->silk.packetLossPercentage = c->packetLossPercentage;
    h->silk.complexity = silk_complexity(c->complexity);
    h->silk.useInBandFEC = c->useInBandFEC;
    h->silk.useDRED = 0;
    h->silk.useDTX = c->useDTX;
    h->silk.useCBR = 0;
    h->silk.maxBits = body * 8;
    h->silk.toMono = 0;
    h->silk.opusCanSwitch = 0;
    h->silk.reducedDependency = 0;
    nbytes = body;
    ec_enc_init(&enc, out + CHUNK_HDR, body);
    if (silk_Encode(silk_state, &h->silk, pcm, samples, &enc, &nbytes, 0, VAD_NO_DECISION)) return SKP_SILK_ENC_INTERNAL_ERROR;
    if (nbytes == 0) {
        used = 0;
    } else {
        used = (ec_tell(&enc) + 7) >> 3;
        ec_enc_done(&enc);
        if (enc.error || used > body) return SKP_SILK_ENC_PAYLOAD_BUF_TOO_SHORT;
    }
    out[0] = (unsigned char)(ms / 10);
    out[1] = (unsigned char)(h->silk.internalSampleRate / 1000);
    out[2] = (unsigned char)(used & 0xff);
    out[3] = (unsigned char)(used >> 8);
    return CHUNK_HDR + used;
}

SKP_API int SKP_Silk_SDK_Encode(void *encState, const SKP_SILK_SDK_EncControlStruct *encControl, const opus_int16 *samplesIn, int nSamplesIn, unsigned char *outData, opus_int16 *nBytesOut) {
    EncHeader *h = (EncHeader *)encState;
    int packet_ms, packet_samples, r, parts[MAX_CHUNKS], nparts, i, pos, off;
    int cap;
    if (!h || h->magic != ENC_MAGIC || !encControl || !nBytesOut) return SKP_SILK_ENC_INTERNAL_ERROR;
    r = validate(encControl, &packet_ms);
    if (r) return r;
    if (nSamplesIn < 0 || (nSamplesIn > 0 && !samplesIn) || (nSamplesIn * 100) % encControl->API_sampleRate) return SKP_SILK_ENC_INPUT_INVALID_NO_OF_SAMPLES;
    if (h->api_rate != encControl->API_sampleRate || h->last.packetSize != encControl->packetSize) {
        h->buffered = 0;
        h->api_rate = encControl->API_sampleRate;
    }
    h->last = *encControl;
    packet_samples = encControl->packetSize;
    if (h->buffered + nSamplesIn > MAX_BUF_SAMPLES || nSamplesIn > packet_samples) return SKP_SILK_ENC_INPUT_INVALID_NO_OF_SAMPLES;
    if (nSamplesIn) memcpy(h->buf + h->buffered, samplesIn, nSamplesIn * sizeof(opus_int16));
    h->buffered += nSamplesIn;
    cap = *nBytesOut;
    if (h->buffered < packet_samples) {
        *nBytesOut = 0;
        return SKP_SILK_NO_ERROR;
    }
    if (!outData || cap <= 0) return SKP_SILK_ENC_PAYLOAD_BUF_TOO_SHORT;
    nparts = split_packet(packet_ms, parts);
    pos = 0;
    off = 0;
    for (i = 0; i < nparts; i++) {
        r = encode_chunk(h, enc_silk(encState), encControl, parts[i], h->buf + off, outData + pos, cap - pos);
        if (r < 0) {
            *nBytesOut = 0;
            return r;
        }
        pos += r;
        off += encControl->API_sampleRate * parts[i] / 1000;
    }
    h->buffered -= packet_samples;
    if (h->buffered) memmove(h->buf, h->buf + packet_samples, h->buffered * sizeof(opus_int16));
    *nBytesOut = (opus_int16)pos;
    return SKP_SILK_NO_ERROR;
}

SKP_API int SKP_Silk_SDK_Get_Decoder_Size(opus_int32 *decSizeBytes) {
    opus_int n = 0;
    if (!decSizeBytes) return SKP_SILK_DEC_PAYLOAD_ERROR;
    if (silk_Get_Decoder_Size(&n)) return SKP_SILK_DEC_PAYLOAD_ERROR;
    *decSizeBytes = header_size(sizeof(DecHeader)) + n;
    return SKP_SILK_NO_ERROR;
}

SKP_API int SKP_Silk_SDK_InitDecoder(void *decState) {
    DecHeader *h = (DecHeader *)decState;
    if (!decState) return SKP_SILK_DEC_PAYLOAD_ERROR;
    memset(h, 0, sizeof(DecHeader));
    if (silk_InitDecoder(dec_silk(decState))) return SKP_SILK_DEC_PAYLOAD_ERROR;
    h->magic = DEC_MAGIC;
    h->internal_rate = 16000;
    return SKP_SILK_NO_ERROR;
}

static int parse_packet(DecHeader *h, const unsigned char *in, int n) {
    int pos = 0;
    h->chunk_count = 0;
    h->frames_total = 0;
    while (pos < n) {
        int ms, khz, len;
        if (h->chunk_count >= MAX_CHUNKS || n - pos < CHUNK_HDR) return SKP_SILK_DEC_PAYLOAD_ERROR;
        ms = in[pos] * 10;
        khz = in[pos + 1];
        len = in[pos + 2] | (in[pos + 3] << 8);
        if ((ms != 20 && ms != 40 && ms != 60) || (khz != 8 && khz != 12 && khz != 16) || len > n - pos - CHUNK_HDR) return SKP_SILK_DEC_PAYLOAD_ERROR;
        h->chunk_ms[h->chunk_count] = ms;
        h->chunk_rate[h->chunk_count] = khz * 1000;
        h->chunk_off[h->chunk_count] = pos + CHUNK_HDR;
        h->chunk_len[h->chunk_count] = len;
        h->frames_total += ms / 20;
        h->chunk_count++;
        pos += CHUNK_HDR + len;
    }
    if (h->chunk_count == 0) return SKP_SILK_DEC_PAYLOAD_ERROR;
    memcpy(h->pkt, in, n);
    h->pkt_len = n;
    h->chunk = 0;
    h->chunk_started = 0;
    h->chunk_frames_left = h->chunk_ms[0] / 20;
    h->frames_left = h->frames_total;
    return SKP_SILK_NO_ERROR;
}

static int decode_frame(DecHeader *h, void *silk_state, int lost, int first, opus_int16 *out, opus_int32 *got) {
    h->silk.nChannelsAPI = 1;
    h->silk.nChannelsInternal = 1;
    h->silk.API_sampleRate = h->api_rate;
    h->silk.internalSampleRate = h->internal_rate;
    h->silk.enable_deep_plc = 0;
    return silk_Decode(silk_state, &h->silk, lost, first, &h->dec, out, got, 0);
}

static int conceal(DecHeader *h, void *silk_state, opus_int16 *out, opus_int32 *total) {
    int want = h->api_rate / 50;
    *total = 0;
    h->silk.payloadSize_ms = 20;
    ec_dec_init(&h->dec, NULL, 0);
    while (*total < want) {
        opus_int32 got = 0;
        if (decode_frame(h, silk_state, 1, *total == 0, out + *total, &got) || got <= 0) {
            memset(out + *total, 0, (want - *total) * sizeof(opus_int16));
            got = want - *total;
        }
        *total += got;
    }
    return SKP_SILK_NO_ERROR;
}

SKP_API int SKP_Silk_SDK_Decode(void *decState, SKP_SILK_SDK_DecControlStruct *decControl, int lostFlag, const unsigned char *inData, const int nBytesIn, opus_int16 *samplesOut, opus_int16 *nSamplesOut) {
    DecHeader *h = (DecHeader *)decState;
    void *silk_state;
    opus_int32 got = 0;
    int r;
    if (!h || h->magic != DEC_MAGIC || !decControl || !samplesOut || !nSamplesOut) return SKP_SILK_DEC_PAYLOAD_ERROR;
    if (!rate_ok(decControl->API_sampleRate)) return SKP_SILK_DEC_INVALID_SAMPLING_FREQUENCY;
    silk_state = dec_silk(decState);
    if (h->api_rate != decControl->API_sampleRate) {
        h->api_rate = decControl->API_sampleRate;
        h->frames_left = 0;
    }
    decControl->inBandFECOffset = 0;
    if (lostFlag || (h->frames_left == 0 && nBytesIn <= 0)) {
        h->frames_left = 0;
        conceal(h, silk_state, samplesOut, &got);
        *nSamplesOut = (opus_int16)got;
        decControl->frameSize = got;
        decControl->framesPerPacket = 1;
        decControl->moreInternalDecoderFrames = 0;
        return SKP_SILK_NO_ERROR;
    }
    if (h->frames_left == 0) {
        if (!inData) return SKP_SILK_DEC_PAYLOAD_ERROR;
        if (nBytesIn > MAX_PACKET_BYTES) return SKP_SILK_DEC_PAYLOAD_TOO_LARGE;
        r = parse_packet(h, inData, nBytesIn);
        if (r) return r;
    }
    if (h->chunk_len[h->chunk] == 0) {
        conceal(h, silk_state, samplesOut, &got);
    } else {
        int first = !h->chunk_started;
        if (first) {
            ec_dec_init(&h->dec, h->pkt + h->chunk_off[h->chunk], h->chunk_len[h->chunk]);
            h->internal_rate = h->chunk_rate[h->chunk];
            h->silk.payloadSize_ms = h->chunk_ms[h->chunk];
            h->chunk_started = 1;
        }
        if (decode_frame(h, silk_state, 0, first, samplesOut, &got) || got <= 0) {
            h->frames_left = 0;
            *nSamplesOut = 0;
            return SKP_SILK_DEC_PAYLOAD_ERROR;
        }
    }
    h->frames_left--;
    h->chunk_frames_left--;
    if (h->chunk_frames_left == 0 && h->chunk + 1 < h->chunk_count) {
        h->chunk++;
        h->chunk_started = 0;
        h->chunk_frames_left = h->chunk_ms[h->chunk] / 20;
    }
    *nSamplesOut = (opus_int16)got;
    decControl->frameSize = got;
    decControl->framesPerPacket = h->frames_total;
    decControl->moreInternalDecoderFrames = h->frames_left > 0;
    return SKP_SILK_NO_ERROR;
}

SKP_API const char *SKP_Silk_SDK_get_version(void) {
    return "1.0.9-tombox-libopus-316c675fa1c9";
}
