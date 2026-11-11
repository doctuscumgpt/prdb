#include "prdb_ipc_frame_v1.h"
#include "prdb_query_abi_payload_codecs.h"

#include <string.h>

static int operation_is_known(uint16_t operation) {
    switch (operation) {
#define PRDB_IPC_OPERATION_CASE(operation_code) case operation_code:
#include "prdb_ipc_operation_cases_v1.h"
#undef PRDB_IPC_OPERATION_CASE
        return 1;
    default:
        return 0;
    }
}

int32_t prdb_ipc_frame_encode_v1(const prdb_ipc_frame_v1 *frame,
                                 uint8_t *output,
                                 uint64_t output_capacity,
                                 uint64_t *output_len) {
    if (frame == NULL || output_len == NULL || (output == NULL && output_capacity != 0)) {
        return PRDB_IPC_FRAME_INVALID_ARGUMENT;
    }
    *output_len = 0;
    if (frame->operation == 0) {
        return PRDB_IPC_FRAME_INVALID_OPERATION;
    }
    if (!operation_is_known(frame->operation)) {
        return PRDB_IPC_FRAME_UNKNOWN_OPERATION;
    }
    if (frame->request_id == 0) {
        return PRDB_IPC_FRAME_INVALID_REQUEST_ID;
    }
    if (frame->payload_len > PRDB_IPC_FRAME_MAX_PAYLOAD_BYTES) {
        return PRDB_IPC_FRAME_PAYLOAD_TOO_LARGE;
    }
    if (frame->payload_len != 0 && frame->payload == NULL) {
        return PRDB_IPC_FRAME_INVALID_ARGUMENT;
    }
    uint64_t required = PRDB_IPC_FRAME_HEADER_LEN + (uint64_t)frame->payload_len;
    *output_len = required;
    if (output == NULL || output_capacity < required) {
        return PRDB_IPC_FRAME_OUTPUT_TOO_SMALL;
    }

    if (frame->payload_len != 0) {
        memmove(output + PRDB_IPC_FRAME_HEADER_LEN, frame->payload, frame->payload_len);
    }
    prdb_encode_ipc_frame_header_record(
        output, PRDB_IPC_FRAME_MAGIC, PRDB_IPC_FRAME_VERSION, frame->operation,
        frame->request_id, frame->payload_len);
    return PRDB_IPC_FRAME_OK;
}

int32_t prdb_ipc_frame_decode_v1(const uint8_t *input,
                                 uint64_t input_len,
                                 prdb_ipc_frame_v1 *frame,
                                 uint64_t *consumed_len) {
    if (input == NULL || frame == NULL || consumed_len == NULL) {
        return PRDB_IPC_FRAME_INVALID_ARGUMENT;
    }
    if (input_len < PRDB_IPC_FRAME_HEADER_LEN) {
        return PRDB_IPC_FRAME_TRUNCATED;
    }
    uint32_t magic = 0;
    uint16_t version = 0;
    uint16_t operation = 0;
    uint64_t request_id = 0;
    uint32_t payload_len = 0;
    if (!prdb_decode_ipc_frame_header_record(
            input, PRDB_IPC_FRAME_HEADER_LEN, &magic, &version, &operation,
            &request_id, &payload_len)) {
        return PRDB_IPC_FRAME_TRUNCATED;
    }
    if (magic != PRDB_IPC_FRAME_MAGIC) {
        return PRDB_IPC_FRAME_INVALID_MAGIC;
    }
    if (version != PRDB_IPC_FRAME_VERSION) {
        return PRDB_IPC_FRAME_UNSUPPORTED_VERSION;
    }
    if (operation == 0) {
        return PRDB_IPC_FRAME_INVALID_OPERATION;
    }
    if (!operation_is_known(operation)) {
        return PRDB_IPC_FRAME_UNKNOWN_OPERATION;
    }
    if (request_id == 0) {
        return PRDB_IPC_FRAME_INVALID_REQUEST_ID;
    }
    if (payload_len > PRDB_IPC_FRAME_MAX_PAYLOAD_BYTES) {
        return PRDB_IPC_FRAME_PAYLOAD_TOO_LARGE;
    }
    uint64_t total_len = PRDB_IPC_FRAME_HEADER_LEN + (uint64_t)payload_len;
    if (input_len < total_len) {
        return PRDB_IPC_FRAME_TRUNCATED;
    }

    prdb_ipc_frame_v1 decoded = {
        operation,
        request_id,
        input + PRDB_IPC_FRAME_HEADER_LEN,
        payload_len,
    };
    *frame = decoded;
    *consumed_len = total_len;
    return PRDB_IPC_FRAME_OK;
}
