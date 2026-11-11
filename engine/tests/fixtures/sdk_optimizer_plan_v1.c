#include "prdb_sdk_optimizer_plan_v1.h"
#include "prdb_sdk_optimizer_v2.h"
#include "prdb_query_abi_payload_codecs.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

static void put_u16(uint8_t *bytes, uint32_t *offset, uint16_t value) {
    bytes[(*offset)++] = (uint8_t)(value >> 8);
    bytes[(*offset)++] = (uint8_t)value;
}

static void put_u32(uint8_t *bytes, uint32_t *offset, uint32_t value) {
    bytes[(*offset)++] = (uint8_t)(value >> 24);
    bytes[(*offset)++] = (uint8_t)(value >> 16);
    bytes[(*offset)++] = (uint8_t)(value >> 8);
    bytes[(*offset)++] = (uint8_t)value;
}

static void put_u64(uint8_t *bytes, uint32_t *offset, uint64_t value) {
    put_u32(bytes, offset, (uint32_t)(value >> 32));
    put_u32(bytes, offset, (uint32_t)value);
}

static uint32_t put_column(uint8_t *bytes, uint32_t *offset,
                           uint8_t type_code, uint8_t nullable) {
    uint32_t id_offset = *offset;
    bytes[(*offset)++] = 3;
    memset(bytes + *offset, 0, 15);
    *offset += 15;
    put_u32(bytes, offset, 3);
    bytes[(*offset)++] = type_code;
    memset(bytes + *offset, 0, 4);
    *offset += 4;
    bytes[(*offset)++] = nullable;
    return id_offset;
}

static void put_name(uint8_t *bytes, uint32_t *offset, const char *name) {
    uint8_t length = (uint8_t)strlen(name);
    bytes[(*offset)++] = length;
    memcpy(bytes + *offset, name, length);
    *offset += length;
}

static void set_column_ordinal(uint8_t *bytes, uint32_t column_offset,
                               uint32_t ordinal) {
    bytes[column_offset + 16] = (uint8_t)(ordinal >> 24);
    bytes[column_offset + 17] = (uint8_t)(ordinal >> 16);
    bytes[column_offset + 18] = (uint8_t)(ordinal >> 8);
    bytes[column_offset + 19] = (uint8_t)ordinal;
}

static uint32_t build_join_plan(uint8_t *bytes, uint32_t *projection_side_offset,
                                uint32_t *right_key_type_offset) {
    uint32_t offset = 0;
    uint8_t identifier[16];
    prdb_encode_optimizer_plan_header_record(bytes + offset, 1, 4, 0);
    offset += PRDB_OPTIMIZER_PLAN_HEADER_RECORD_HEADER_BYTES;
    put_name(bytes, &offset, "public");
    memset(identifier, 1, sizeof(identifier));
    prdb_encode_optimizer_plan_schema_reference_record(bytes + offset,
                                                       identifier, 3);
    offset += PRDB_OPTIMIZER_PLAN_SCHEMA_REFERENCE_RECORD_HEADER_BYTES;
    for (uint32_t side = 0; side < 2; ++side) {
        put_name(bytes, &offset, side == 0 ? "lefts" : "rights");
        memset(identifier, side == 0 ? 0x31 : 0x32, sizeof(identifier));
        prdb_encode_optimizer_plan_relation_reference_record(
            bytes + offset, identifier, side == 0 ? 4 : 5);
        offset += PRDB_OPTIMIZER_PLAN_RELATION_REFERENCE_RECORD_HEADER_BYTES;
    }
    for (uint32_t side = 0; side < 2; ++side) {
        put_u16(bytes, &offset, 3);
        uint32_t column = put_column(bytes, &offset, 3, 0);
        bytes[column] = side == 0 ? 0x41 : 0x51;
        set_column_ordinal(bytes, column, 0);
        column = put_column(bytes, &offset, 1, 0);
        bytes[column] = side == 0 ? 0x42 : 0x52;
        set_column_ordinal(bytes, column, 1);
        column = put_column(bytes, &offset, 6, 1);
        bytes[column] = side == 0 ? 0x43 : 0x53;
        set_column_ordinal(bytes, column, 2);
    }
    put_u16(bytes, &offset, 2);
    *projection_side_offset = offset;
    bytes[offset++] = 0;
    uint32_t column = put_column(bytes, &offset, 6, 1);
    bytes[column] = 0x43;
    set_column_ordinal(bytes, column, 2);
    bytes[offset++] = 1;
    column = put_column(bytes, &offset, 6, 1);
    bytes[column] = 0x53;
    set_column_ordinal(bytes, column, 2);
    bytes[offset++] = 2;
    column = put_column(bytes, &offset, 3, 0);
    bytes[column] = 0x41;
    set_column_ordinal(bytes, column, 0);
    column = put_column(bytes, &offset, 3, 0);
    bytes[column] = 0x51;
    set_column_ordinal(bytes, column, 0);
    *right_key_type_offset = column + 20;
    column = put_column(bytes, &offset, 1, 0);
    bytes[column] = 0x42;
    set_column_ordinal(bytes, column, 1);
    column = put_column(bytes, &offset, 1, 0);
    bytes[column] = 0x52;
    set_column_ordinal(bytes, column, 1);
    bytes[offset++] = 1;
    put_u32(bytes, &offset, 5);
    return offset;
}

static uint32_t put_header(uint8_t *bytes, uint32_t *offset,
                           uint8_t node_count) {
    bytes[(*offset)++] = 0;
    bytes[(*offset)++] = 1; /* version */
    bytes[(*offset)++] = 1; /* SELECT */
    bytes[(*offset)++] = 0; /* reserved */
    bytes[(*offset)++] = 6;
    memcpy(bytes + *offset, "public", 6);
    *offset += 6;
    bytes[(*offset)++] = 1;
    memset(bytes + *offset, 0, 15);
    *offset += 15;
    put_u64(bytes, offset, 1); /* schema version */
    bytes[(*offset)++] = 5;
    memcpy(bytes + *offset, "items", 5);
    *offset += 5;
    bytes[(*offset)++] = 2;
    memset(bytes + *offset, 0, 15);
    *offset += 15;
    put_u64(bytes, offset, 1); /* table epoch */
    bytes[(*offset)++] = node_count;
    return *offset;
}

static uint32_t build_plan(uint8_t *bytes, uint32_t *projected_id_offset,
                           uint32_t *project_node_offset) {
    uint32_t offset = 0;
    (void)put_header(bytes, &offset, 3);
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    (void)put_column(bytes, &offset, 6, 1);
    *project_node_offset = offset;
    bytes[offset++] = 3;
    put_u16(bytes, &offset, 1);
    *projected_id_offset = put_column(bytes, &offset, 6, 1);
    bytes[offset++] = 4;
    put_u32(bytes, &offset, 7);
    return offset;
}

static void put_integer_comparison(uint8_t *bytes, uint32_t *offset,
                                   uint8_t integer_value) {
    bytes[(*offset)++] = 0x20;
    bytes[(*offset)++] = 0;
    bytes[(*offset)++] = 0x10;
    bytes[(*offset)++] = 10;
    memset(bytes + *offset, 0, 15);
    *offset += 15;
    bytes[(*offset)++] = 0x02;
    memset(bytes + *offset, 0, 7);
    *offset += 7;
    bytes[(*offset)++] = integer_value;
}

static uint32_t build_filter_plan(uint8_t *bytes,
                                  uint32_t *predicate_offset) {
    uint32_t offset = 0;
    (void)put_header(bytes, &offset, 4);
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    uint32_t column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0; /* ordinal 0 */
    bytes[offset++] = 2;
    *predicate_offset = offset;
    /* Comparison predicate: scanned column equals parameter value one. */
    put_integer_comparison(bytes, &offset, 1);
    bytes[offset++] = 3;
    put_u16(bytes, &offset, 1);
    column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0; /* ordinal 0 */
    bytes[offset++] = 4;
    put_u32(bytes, &offset, 1);
    return offset;
}

static uint32_t build_boolean_filter_plan(uint8_t *bytes,
                                          uint32_t *predicate_offset) {
    uint32_t offset = 0;
    (void)put_header(bytes, &offset, 4);
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    uint32_t column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0;
    bytes[offset++] = 2;
    *predicate_offset = offset;
    bytes[offset++] = 0x30;
    bytes[offset++] = 2;
    put_integer_comparison(bytes, &offset, 1);
    put_integer_comparison(bytes, &offset, 1);
    bytes[offset++] = 3;
    put_u16(bytes, &offset, 1);
    column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0;
    bytes[offset++] = 4;
    put_u32(bytes, &offset, 1);
    return offset;
}

static uint32_t build_delete_plan(uint8_t *bytes,
                                  uint32_t *predicate_offset) {
    uint32_t offset = 0;
    (void)put_header(bytes, &offset, 3);
    bytes[2] = 2; /* DELETE */
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 0); /* constant predicate needs no scan columns */
    bytes[offset++] = 2;
    *predicate_offset = offset;
    bytes[offset++] = 0x01;
    bytes[offset++] = 1;
    bytes[offset++] = 5; /* Delete */
    return offset;
}

static uint32_t build_delete_filter_plan(uint8_t *bytes,
                                         uint32_t *predicate_offset) {
    uint32_t offset = 0;
    uint32_t column_id_offset;
    (void)put_header(bytes, &offset, 3);
    bytes[2] = 2; /* DELETE */
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10; /* c0 */
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0; /* ordinal 0 */
    bytes[offset++] = 2;
    *predicate_offset = offset;
    put_integer_comparison(bytes, &offset, 1);
    bytes[offset++] = 5; /* Delete */
    return offset;
}

static uint32_t build_update_plan(uint8_t *bytes) {
    uint32_t offset = 0;
    uint32_t column_id_offset;
    (void)put_header(bytes, &offset, 3);
    bytes[2] = 3; /* UPDATE */
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0;
    bytes[offset++] = 2;
    put_integer_comparison(bytes, &offset, 1);
    prdb_encode_optimizer_plan_update_node_record(
        bytes + offset, PRDB_OPTIMIZER_PLAN_UPDATE_NODE, 1);
    offset += PRDB_OPTIMIZER_PLAN_UPDATE_NODE_RECORD_HEADER_BYTES;
    column_id_offset = put_column(bytes, &offset, 3, 0);
    bytes[column_id_offset] = 10;
    bytes[column_id_offset + 16] = 0;
    bytes[column_id_offset + 17] = 0;
    bytes[column_id_offset + 18] = 0;
    bytes[column_id_offset + 19] = 0;
    prdb_encode_optimizer_plan_update_assignment_value_record(
        bytes + offset, PRDB_OPTIMIZER_UPDATE_PARAMETER_VALUE, 2);
    offset += PRDB_OPTIMIZER_PLAN_UPDATE_ASSIGNMENT_VALUE_RECORD_HEADER_BYTES;
    return offset;
}

typedef struct resolve_context {
    uint8_t response[512];
    uint32_t response_len;
    uint8_t executor_response[1024];
    uint32_t executor_response_len;
    uint8_t scan_response[18];
    uint8_t batch_response[128];
    uint32_t batch_response_len;
    uint8_t cancellation_response[3];
    uint8_t close_response[3];
    uint8_t emitted_result[256];
    uint32_t emitted_result_len;
    uint32_t fail_result_write;
    uint32_t result_write_calls;
    uint8_t optimizer_response[512];
    uint32_t optimizer_response_len;
    uint32_t calls;
    uint32_t optimizer_calls;
    uint32_t fail_optimizer_request;
    uint32_t fail_join_identity;
    uint32_t stage_delete_calls;
    uint32_t stage_update_calls;
    uint32_t fail_stage_update;
    uint32_t statement_failures;
    uint16_t events[32];
    uint32_t event_count;
} resolve_context;

static int32_t optimizer_request(void *context, const uint8_t *request,
                                 uint64_t request_len, uint8_t *response,
                                 uint64_t response_capacity,
                                 uint64_t *response_len) {
    resolve_context *resolved = (resolve_context *)context;
    assert(request_len >= 16 && request[0] == 0 && request[1] == 1);
    if (resolved->event_count < 32) {
        resolved->events[resolved->event_count++] = UINT16_C(0xffff);
    }
    resolved->optimizer_calls++;
    if (resolved->fail_optimizer_request != 0)
        return PRDB_QUERY_CALLBACK_INTERNAL_ERROR;
    if (response_capacity < resolved->optimizer_response_len) {
        *response_len = resolved->optimizer_response_len;
        return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
    }
    memcpy(response, resolved->optimizer_response,
           resolved->optimizer_response_len);
    *response_len = resolved->optimizer_response_len;
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t optimizer_request_v5(
    void *context, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    assert(operation == PRDB_IPC_OP_OPTIMIZE_DELETE ||
           operation == PRDB_IPC_OP_OPTIMIZE_UPDATE);
    assert(request_len >= PRDB_OPTIMIZER_SELECT_REQUEST_HEADER_BYTES);
    assert(request[2] == (operation == PRDB_IPC_OP_OPTIMIZE_DELETE
                              ? PRDB_OPTIMIZER_DELETE_STATEMENT
                              : PRDB_OPTIMIZER_UPDATE_STATEMENT));
    return optimizer_request(context, request, request_len, response,
                             response_capacity, response_len);
}

static int32_t statement_failure(void *context, uint32_t failure_code) {
    resolve_context *resolved = (resolve_context *)context;
    assert(failure_code == PRDB_QUERY_STATEMENT_FAILURE_SQL);
    resolved->statement_failures++;
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t write_result(void *context, const uint8_t *bytes,
                            uint64_t bytes_len) {
    resolve_context *resolved = (resolve_context *)context;
    resolved->result_write_calls++;
    if (resolved->fail_result_write != 0)
        return PRDB_QUERY_CALLBACK_INTERNAL_ERROR;
    if (bytes_len > sizeof(resolved->emitted_result) -
                        resolved->emitted_result_len) {
        return PRDB_QUERY_CALLBACK_LIMIT_EXCEEDED;
    }
    memcpy(resolved->emitted_result + resolved->emitted_result_len, bytes,
           (size_t)bytes_len);
    resolved->emitted_result_len += (uint32_t)bytes_len;
    return PRDB_QUERY_CALLBACK_OK;
}

static uint32_t build_join_relation_response(uint8_t *bytes, uint32_t side);

static int32_t resolve_plan_relation(void *context, uint16_t operation,
                                     const uint8_t *request,
                                     uint64_t request_len, uint8_t *response,
                                     uint64_t response_capacity,
                                     uint64_t *response_len) {
    resolve_context *resolved = (resolve_context *)context;
    const uint8_t *source;
    uint32_t source_len;
    uint8_t join_relation_response[512];
    int32_t callback_status = PRDB_QUERY_CALLBACK_OK;
    if (operation == PRDB_IPC_OP_RESOLVE_PLAN_RELATION) {
        assert((request_len == 17 || request_len == 18) &&
               request[0] == 0 && request[1] == 1);
        assert(request[2] == 0 && request[3] == 6);
        assert(memcmp(request + 6, "public", 6) == 0);
        assert(request[4] == 0);
        if (request[5] == 5 && memcmp(request + 12, "items", 5) == 0) {
            source = resolved->response;
            source_len = resolved->response_len;
        } else if (request[5] == 5 &&
                   memcmp(request + 12, "lefts", 5) == 0) {
            source_len = build_join_relation_response(join_relation_response, 0);
            source = join_relation_response;
        } else {
            assert(request[5] == 6 && request_len == 18 &&
                   memcmp(request + 12, "rights", 6) == 0);
            source_len = build_join_relation_response(join_relation_response, 1);
            if (resolved->fail_join_identity != 0)
                join_relation_response[34] ^= 1;
            source = join_relation_response;
        }
    } else if (operation == PRDB_SDK_EXECUTOR_OP_RESOLVE_RELATION) {
        assert(request_len == 17 && request[0] == 0 && request[1] == 1);
        assert(request[2] == 0 && request[3] == 6);
        assert(memcmp(request + 6, "public", 6) == 0);
        assert(request[4] == 0 && request[5] == 5);
        assert(memcmp(request + 12, "items", 5) == 0);
        source = resolved->executor_response;
        source_len = resolved->executor_response_len;
    } else if (operation == PRDB_SDK_EXECUTOR_OP_OPEN_SCAN) {
        assert(request_len == 20 && request[0] == 0 && request[1] == 1);
        assert(request[2] == 6);
        assert(request[18] == 0 && request[19] == 0);
        source = resolved->scan_response;
        source_len = sizeof(resolved->scan_response);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_CHECK_CANCELLATION) {
        assert(request_len == 0);
        source = resolved->cancellation_response;
        source_len = sizeof(resolved->cancellation_response);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_NEXT_BATCH) {
        assert(request_len == 24 && request[2] == 7);
        source = resolved->batch_response;
        source_len = resolved->batch_response_len;
    } else if (operation == PRDB_SDK_EXECUTOR_OP_STAGE_DELETE) {
        static const uint8_t acknowledgement[] = {0, 1, 0};
        assert(request_len == 18 && request[0] == 0 && request[1] == 1);
        assert(request[2] == 8);
        ++resolved->stage_delete_calls;
        source = acknowledgement;
        source_len = sizeof(acknowledgement);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_STAGE_UPDATE) {
        static const uint8_t acknowledgement[] = {0, 1, 0};
        assert(request_len == 34 && request[0] == 0 && request[1] == 1);
        assert(request[2] == 8 && request[18] == 0 && request[19] == 1);
        assert(request[20] == 0 && request[21] == 0 &&
               request[22] == 0 && request[23] == 0);
        assert(request[24] == 0 &&
               request[25] == PRDB_SDK_VALUE_TYPE_INTEGER);
        ++resolved->stage_update_calls;
        if (resolved->fail_stage_update != 0)
            callback_status = PRDB_QUERY_CALLBACK_INTERNAL_ERROR;
        source = acknowledgement;
        source_len = sizeof(acknowledgement);
    } else {
        assert(operation == PRDB_SDK_EXECUTOR_OP_CLOSE_CURSOR);
        assert(request_len == 18 && request[2] == 7);
        source = resolved->close_response;
        source_len = sizeof(resolved->close_response);
    }
    resolved->calls++;
    if (resolved->event_count < 32) {
        resolved->events[resolved->event_count++] = operation;
    }
    if (callback_status != PRDB_QUERY_CALLBACK_OK) return callback_status;
    if (response_capacity < source_len) {
        *response_len = source_len;
        return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
    }
    memcpy(response, source, source_len);
    *response_len = source_len;
    return PRDB_QUERY_CALLBACK_OK;
}

static uint32_t build_relation_response(uint8_t *bytes) {
    uint32_t offset = 0;
    bytes[offset++] = 0;
    bytes[offset++] = 1;
    bytes[offset++] = 0;
    bytes[offset++] = 1;
    memset(bytes + offset, 0, 15);
    offset += 15;
    put_u64(bytes, &offset, 1);
    put_u64(bytes, &offset, 1);
    bytes[offset++] = 2;
    memset(bytes + offset, 0, 15);
    offset += 15;
    put_u16(bytes, &offset, 5);
    memcpy(bytes + offset, "items", 5);
    offset += 5;
    put_u16(bytes, &offset, 4);
    for (uint32_t ordinal = 0; ordinal < 4; ++ordinal) {
        bytes[offset++] = ordinal == 3 ? 3 : (uint8_t)(10 + ordinal);
        memset(bytes + offset, 0, 15);
        offset += 15;
        put_u32(bytes, &offset, ordinal);
        bytes[offset++] = ordinal == 3 ? 1 : 0;
        bytes[offset++] = ordinal == 3 ? 6 : 3;
        memset(bytes + offset, 0, 4);
        offset += 4;
        put_u16(bytes, &offset, 2);
        bytes[offset++] = 'c';
        bytes[offset++] = (uint8_t)('0' + ordinal);
    }
    return offset;
}

static uint32_t build_join_relation_response(uint8_t *bytes, uint32_t side) {
    uint32_t offset = 0;
    const char *name = side == 0 ? "lefts" : "rights";
    uint8_t relation_id = side == 0 ? 0x31 : 0x32;
    uint8_t column_base = side == 0 ? 0x41 : 0x51;
    bytes[offset++] = 0;
    bytes[offset++] = 1;
    bytes[offset++] = 0;
    memset(bytes + offset, 1, 16);
    offset += 16;
    put_u64(bytes, &offset, 3);
    put_u64(bytes, &offset, side == 0 ? 4 : 5);
    memset(bytes + offset, relation_id, 16);
    offset += 16;
    put_u16(bytes, &offset, (uint16_t)strlen(name));
    memcpy(bytes + offset, name, strlen(name));
    offset += (uint32_t)strlen(name);
    put_u16(bytes, &offset, 3);
    for (uint32_t ordinal = 0; ordinal < 3; ++ordinal) {
        bytes[offset++] = (uint8_t)(column_base + ordinal);
        memset(bytes + offset, 0, 15);
        offset += 15;
        put_u32(bytes, &offset, ordinal);
        bytes[offset++] = ordinal == 2 ? 1 : 0;
        bytes[offset++] = ordinal == 0 ? 3 : ordinal == 1 ? 1 : 6;
        memset(bytes + offset, 0, 4);
        offset += 4;
        put_u16(bytes, &offset,
                (uint16_t)(ordinal == 0 ? 2 : ordinal == 1 ? 6 : 5));
        if (ordinal == 0) {
            memcpy(bytes + offset, "id", 2);
        } else if (ordinal == 1) {
            memcpy(bytes + offset, "active", 6);
        } else {
            memcpy(bytes + offset, "value", 5);
        }
        offset += ordinal == 0 ? 2 : ordinal == 1 ? 6 : 5;
    }
    return offset;
}

static uint32_t build_executor_relation_response(uint8_t *bytes) {
    uint32_t offset = 0;
    bytes[offset++] = 0;
    bytes[offset++] = 1;
    bytes[offset++] = 0;
    bytes[offset++] = 2;
    memset(bytes + offset, 0, 15);
    offset += 15;
    bytes[offset++] = 6;
    memset(bytes + offset, 0, 15);
    offset += 15;
    put_u16(bytes, &offset, 5);
    memcpy(bytes + offset, "items", 5);
    offset += 5;
    put_u16(bytes, &offset, 4);
    for (uint32_t ordinal = 0; ordinal < 4; ++ordinal) {
        bytes[offset++] = ordinal == 3 ? 3 : (uint8_t)(10 + ordinal);
        memset(bytes + offset, 0, 15);
        offset += 15;
        put_u32(bytes, &offset, ordinal);
        bytes[offset++] = ordinal == 3 ? 1 : 0;
        bytes[offset++] = ordinal == 3 ? 6 : 3;
        memset(bytes + offset, 0, 4);
        offset += 4;
        put_u16(bytes, &offset, 2);
        bytes[offset++] = 'c';
        bytes[offset++] = (uint8_t)('0' + ordinal);
    }
    return offset;
}

static uint32_t build_row_batch(uint8_t *bytes) {
    uint32_t offset = 0;
    put_u16(bytes, &offset, 1);
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 1);
    bytes[offset++] = 8;
    memset(bytes + offset, 0, 15);
    offset += 15;
    put_u16(bytes, &offset, 4);
    for (uint32_t ordinal = 0; ordinal < 4; ++ordinal) {
        bytes[offset++] = PRDB_SDK_VALUE_STATE_VALUE;
        bytes[offset++] = ordinal == 3 ? PRDB_SDK_VALUE_TYPE_TEXT
                                       : PRDB_SDK_VALUE_TYPE_INTEGER;
        put_u32(bytes, &offset, ordinal == 3 ? 1 : 4);
        if (ordinal == 3) {
            bytes[offset++] = 'x';
        } else {
            memset(bytes + offset, 0, 3);
            bytes[offset + 3] = (uint8_t)(ordinal + 1);
            offset += 4;
        }
    }
    return offset;
}

static uint32_t build_two_row_batch(uint8_t *bytes) {
    uint32_t length = build_row_batch(bytes);
    memcpy(bytes + length, bytes + 5, length - 5);
    bytes[4] = 2;
    bytes[length + 1] = 9;
    return length * 2 - 5;
}

static uint32_t build_continuing_row_batch(uint8_t *bytes) {
    uint32_t length = build_row_batch(bytes);
    memmove(bytes + 19, bytes + 3, length - 3);
    bytes[2] = 0;
    bytes[3] = 7;
    memset(bytes + 4, 0, 15);
    return length + 16;
}

static uint32_t build_optimizer_response(uint8_t *bytes,
                                         const uint8_t *plan,
                                         uint32_t plan_len) {
    uint32_t offset = 0;
    put_u16(bytes, &offset, 1);
    bytes[offset++] = PRDB_SDK_OPTIMIZER_STATUS_SUCCESS;
    put_u32(bytes, &offset, plan_len);
    memcpy(bytes + offset, plan, plan_len);
    return offset + plan_len;
}

static uint32_t projected_rows_seen;

static int32_t on_projected_row(
    void *context, const prdb_sdk_optimizer_plan_column_v1 *projections,
    const prdb_sdk_value_v1 *values, uint32_t projection_count) {
    (void)context;
    assert(projection_count == 1);
    if (projections[0].ordinal == 3) {
        assert(values[0].type_code == PRDB_SDK_VALUE_TYPE_TEXT);
        assert(values[0].data_len == 1 && values[0].data[0] == 'x');
    } else {
        assert(projections[0].ordinal == 0);
        assert(values[0].type_code == PRDB_SDK_VALUE_TYPE_INTEGER);
        assert(values[0].data_len == 4 && values[0].data[3] == 1);
    }
    projected_rows_seen++;
    return 0;
}

static int32_t fail_projected_row(
    void *context, const prdb_sdk_optimizer_plan_column_v1 *projections,
    const prdb_sdk_value_v1 *values, uint32_t projection_count) {
    (void)context;
    (void)projections;
    (void)values;
    (void)projection_count;
    return 1;
}

int main(void) {
    uint8_t payload[256] = {0};
    uint8_t malformed[sizeof(payload)];
    uint8_t join_payload[1024] = {0};
    uint8_t join_malformed[sizeof(join_payload)];
    uint32_t projected_id_offset;
    uint32_t project_node_offset;
    uint32_t predicate_offset;
    uint32_t payload_len = build_plan(payload, &projected_id_offset,
                                      &project_node_offset);
    static prdb_sdk_optimizer_plan_v1 decoded;
    static prdb_sdk_optimizer_filter_v1 filter;
    static prdb_sdk_optimizer_update_plan_v1 update_decoded;
    static prdb_sdk_optimizer_join_plan_v1 join_decoded;
    static resolve_context resolve = {0};
    static prdb_host_api_v2 host = {0};
    static prdb_host_api_v4 host_v4 = {0};
    static prdb_host_api_v5 host_v5 = {0};
    static prdb_sdk_executor_relation_v1 acquired_relation;
    static prdb_sdk_sql_column_shape_v2 acquired_columns[4];
    static prdb_sdk_optimizer_scan_state_v1 scan_state;
    static prdb_sdk_batch_v1 row_batch;
    static prdb_sdk_value_v1 source_cells[4];
    static prdb_sdk_value_v1 projected_values[1];
    static prdb_sdk_value_v1 delete_cell_scratch[4];
    static prdb_sdk_select_binding_v2 delete_binding;
    uint8_t row_batch_bytes[128];
    uint8_t batch_response[128];
    uint8_t result_record_buffer[256];
    uint8_t scan_response[18];
    uint32_t row_batch_len;
    uint32_t delivered_rows = 0;
    uint32_t total_rows = 0;
    uint8_t response[512];
    uint8_t request[512];
    uint8_t optimizer_response[512];
    uint8_t delete_target_handles[
        PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS * 16];
    uint8_t sql[] = "SELECT c3 FROM items LIMIT 1";
    static prdb_sdk_select_v2 statement;
    static prdb_sdk_delete_v1 delete_statement;
    static prdb_sdk_update_v1 update_statement;
    uint64_t response_len = 0;
    uint64_t scan_response_len = 0;

    assert(prdb_sdk_optimizer_decode_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(filter.present == 0 && filter.prefix == NULL &&
           filter.prefix_len == 0);
    assert(decoded.struct_size == sizeof(decoded));
    assert(decoded.abi_version == PRDB_SDK_OPTIMIZER_PLAN_ABI_V1_VERSION);
    assert(decoded.schema_name_len == 6);
    assert(memcmp(decoded.schema_name, "public", 6) == 0);
    assert(decoded.relation_name_len == 5);
    assert(memcmp(decoded.relation_name, "items", 5) == 0);
    assert(decoded.schema_version == 1 && decoded.table_epoch == 1);
    assert(decoded.scan_column_count == 1 && decoded.projection_count == 1);
    assert(decoded.scan_columns[0].ordinal == 3);
    assert(decoded.projections[0].type_code == 6);
    assert(decoded.has_limit == 1 && decoded.limit == 7);
    assert(prdb_sdk_optimizer_scan_state_init_v1(
               payload, payload_len, &scan_state, sizeof(scan_state)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    row_batch_len = build_row_batch(row_batch_bytes);
    assert(prdb_sdk_batch_open_v1(&row_batch, sizeof(row_batch), row_batch_bytes,
                                  row_batch_len) == PRDB_SDK_BATCH_OK);
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_consume_scan_batch_v1(
               &scan_state, &row_batch, 4, source_cells, 4, projected_values,
               1, on_projected_row, NULL, &delivered_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(delivered_rows == 1 && projected_rows_seen == 1);
    uint8_t limit_one_plan[sizeof(payload)];
    memcpy(limit_one_plan, payload, payload_len);
    limit_one_plan[payload_len - 1] = 1;
    assert(prdb_sdk_optimizer_scan_state_init_v1(
               limit_one_plan, payload_len, &scan_state, sizeof(scan_state)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(prdb_sdk_optimizer_consume_scan_batch_v1(
               &scan_state, &row_batch, 4, source_cells, 4, projected_values,
               1, on_projected_row, NULL, &delivered_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(delivered_rows == 1 && scan_state.done == 1);
    assert(prdb_sdk_optimizer_consume_scan_batch_v1(
               &scan_state, &row_batch, 4, source_cells, 4, projected_values,
               1, on_projected_row, NULL, &delivered_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(delivered_rows == 0 && projected_rows_seen == 2);
    payload_len = build_filter_plan(payload, &predicate_offset);
    assert(prdb_sdk_optimizer_scan_state_init_v1(
               payload, payload_len, &scan_state, sizeof(scan_state)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    scan_state.projections[0].ordinal = 1;
    assert(prdb_sdk_optimizer_consume_plan_batch_v1(
               payload, payload_len, &scan_state, &row_batch, 4, source_cells,
               4, projected_values, 1, on_projected_row, NULL,
               &delivered_rows) == PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    scan_state.projections[0].ordinal = 0;
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_consume_plan_batch_v1(
               payload, payload_len, &scan_state, &row_batch, 4, source_cells,
               4, projected_values, 1, on_projected_row, NULL,
               &delivered_rows) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(delivered_rows == 1 && projected_rows_seen == 1);
    row_batch_bytes[32] = 2;
    assert(prdb_sdk_batch_open_v1(&row_batch, sizeof(row_batch), row_batch_bytes,
                                  row_batch_len) == PRDB_SDK_BATCH_OK);
    assert(prdb_sdk_optimizer_scan_state_init_v1(
               payload, payload_len, &scan_state, sizeof(scan_state)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(prdb_sdk_optimizer_consume_plan_batch_v1(
               payload, payload_len, &scan_state, &row_batch, 4, source_cells,
               4, projected_values, 1, on_projected_row, NULL,
               &delivered_rows) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(delivered_rows == 0 && projected_rows_seen == 1);
    row_batch_bytes[32] = 1;
    assert(prdb_sdk_batch_open_v1(&row_batch, sizeof(row_batch), row_batch_bytes,
                                  row_batch_len) == PRDB_SDK_BATCH_OK);
    payload_len = build_plan(payload, &projected_id_offset,
                              &project_node_offset);

    resolve.response_len = build_relation_response(resolve.response);
    resolve.executor_response_len = build_executor_relation_response(
        resolve.executor_response);
    resolve.scan_response[1] = 1;
    resolve.scan_response[2] = 7;
    resolve.batch_response_len = build_continuing_row_batch(
        resolve.batch_response);
    resolve.cancellation_response[1] = 1;
    resolve.close_response[1] = 1;
    host.struct_size = sizeof(host);
    host.abi_version = PRDB_QUERY_ABI_V2_VERSION;
    host.context = &resolve;
    host.executor_request = resolve_plan_relation;
    uint8_t filtered_plan[512];
    uint32_t filtered_plan_len = build_filter_plan(filtered_plan,
                                                   &predicate_offset);
    uint8_t sql_filtered[] = "SELECT c0 FROM items WHERE c0 = $1 LIMIT 1";
    uint8_t filter_parameter_bytes[4] = {0, 0, 0, 1};
    prdb_sdk_value_v1 filter_parameters[1] = {{
        sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
        filter_parameter_bytes, sizeof(filter_parameter_bytes)}};
    assert(prdb_sdk_sql_parse_select_v2(
               sql_filtered, sizeof(sql_filtered) - 1, &statement,
               sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_validate_select_plan_with_parameters_v1(
               &host, &statement, filter_parameters, 1,
               (const uint8_t *)"public", 6, filtered_plan,
               filtered_plan_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 1);
    filter_parameter_bytes[3] = 2;
    assert(prdb_sdk_optimizer_validate_select_plan_with_parameters_v1(
               &host, &statement, filter_parameters, 1,
               (const uint8_t *)"public", 6, filtered_plan,
               filtered_plan_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    filter_parameter_bytes[3] = 1;
    resolve.calls = 0;
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_execute_unfiltered_plan_v1(
               &host, filtered_plan, filtered_plan_len, 2,
               sizeof(batch_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL, &total_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(total_rows == 1 && projected_rows_seen == 1 && resolve.calls == 6);
    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len) == PRDB_SDK_OPTIMIZER_PLAN_OK);

    resolve.response[34] = 2; /* A newer table epoch must reject this plan. */
    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    resolve.response[34] = 1;
    resolve.response[150] = 4; /* Scanned column ID differs at ordinal 3. */
    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    resolve.response[150] = 3;
    resolve.response[2] = 1;
    resolve.response_len = 3;
    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len) == PRDB_SDK_OPTIMIZER_PLAN_UNAVAILABLE);
    resolve.response[2] = 0;
    resolve.response_len = build_relation_response(resolve.response);
    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, payload, payload_len, response, 3, &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_RESPONSE_TOO_SMALL);

    memcpy(limit_one_plan, payload, payload_len);
    limit_one_plan[payload_len - 1] = 1;
    projected_rows_seen = 0;
    resolve.calls = 0;
    int32_t execute_status = prdb_sdk_optimizer_execute_unfiltered_plan_v1(
               &host, limit_one_plan, payload_len, 2, sizeof(batch_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows);
    assert(execute_status == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(total_rows == 1 && projected_rows_seen == 1 && resolve.calls == 6);

    assert(prdb_sdk_sql_parse_select_v2(
               sql, sizeof(sql) - 1, &statement, sizeof(statement), NULL) ==
           PRDB_SDK_SQL_OK);
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_validate_select_plan_v1(
               &host, &statement, (const uint8_t *)"public", 6, limit_one_plan,
               payload_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 1);
    uint8_t sql_mixed_case[] = "SELECT C3 FROM ITEMS LIMIT 1";
    assert(prdb_sdk_sql_parse_select_v2(
               sql_mixed_case, sizeof(sql_mixed_case) - 1, &statement,
               sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    assert(prdb_sdk_optimizer_validate_select_plan_v1(
               &host, &statement, (const uint8_t *)"public", 6, limit_one_plan,
               payload_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 2);
    uint8_t sql_wrong_projection[] = "SELECT c2 FROM items LIMIT 1";
    assert(prdb_sdk_sql_parse_select_v2(
               sql_wrong_projection, sizeof(sql_wrong_projection) - 1,
               &statement, sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    uint32_t calls_before_projection_mismatch = resolve.calls;
    assert(prdb_sdk_optimizer_validate_select_plan_v1(
               &host, &statement, (const uint8_t *)"public", 6, limit_one_plan,
               payload_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    assert(resolve.calls == calls_before_projection_mismatch + 1);
    uint8_t sql_wrong_limit[] = "SELECT c3 FROM items LIMIT 2";
    assert(prdb_sdk_sql_parse_select_v2(
               sql_wrong_limit, sizeof(sql_wrong_limit) - 1, &statement,
               sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    calls_before_projection_mismatch = resolve.calls;
    assert(prdb_sdk_optimizer_validate_select_plan_v1(
               &host, &statement, (const uint8_t *)"public", 6, limit_one_plan,
               payload_len, response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    assert(resolve.calls == calls_before_projection_mismatch);
    assert(prdb_sdk_sql_parse_select_v2(
               sql, sizeof(sql) - 1, &statement, sizeof(statement), NULL) ==
           PRDB_SDK_SQL_OK);
    resolve.optimizer_response_len = build_optimizer_response(
        resolve.optimizer_response, limit_one_plan, payload_len);
    host_v4.v3.struct_size = sizeof(host_v4.v3);
    host_v4.v3.abi_version = PRDB_QUERY_ABI_V3_VERSION;
    host_v4.v3.context = &resolve;
    host_v4.v3.executor_request = resolve_plan_relation;
    host_v4.v3.write_result = write_result;
    host_v4.v3.statement_failure = statement_failure;
    host_v4.optimizer_request = optimizer_request;
    resolve.optimizer_calls = 0;
    resolve.calls = 0;
    resolve.event_count = 0;
    resolve.statement_failures = 0;
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_execute_select_ast_v1(
               &host_v4, &statement, (const uint8_t *)"public", 6, 2,
               sizeof(batch_response), request, sizeof(request),
               optimizer_response, sizeof(optimizer_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.optimizer_calls == 1 && resolve.calls == 7);
    assert(resolve.statement_failures == 0 && total_rows == 1 &&
           projected_rows_seen == 1);
    assert(resolve.event_count == 8 && resolve.events[0] == UINT16_C(0xffff));
    assert(resolve.events[1] == PRDB_IPC_OP_RESOLVE_PLAN_RELATION);
    assert(resolve.events[2] == PRDB_IPC_OP_RESOLVE_PLAN_RELATION);
    assert(resolve.events[3] == PRDB_SDK_EXECUTOR_OP_RESOLVE_RELATION);
    assert(resolve.events[4] == PRDB_SDK_EXECUTOR_OP_OPEN_SCAN);
    assert(resolve.events[5] == PRDB_SDK_EXECUTOR_OP_CHECK_CANCELLATION);
    assert(resolve.events[6] == PRDB_SDK_EXECUTOR_OP_NEXT_BATCH);
    assert(resolve.events[7] == PRDB_SDK_EXECUTOR_OP_CLOSE_CURSOR);

    resolve.calls = 0;
    resolve.event_count = 0;
    resolve.emitted_result_len = 0;
    resolve.result_write_calls = 0;
    assert(prdb_sdk_optimizer_execute_select_ast_to_result_v1(
               &host_v4, &statement, (const uint8_t *)"public", 6, 2,
               sizeof(batch_response), request, sizeof(request),
               optimizer_response, sizeof(optimizer_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, result_record_buffer,
               sizeof(result_record_buffer), &total_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(total_rows == 1 && resolve.emitted_result_len > 8);
    assert(memcmp(resolve.emitted_result, "SQLR", 4) == 0);
    assert(resolve.emitted_result[4] == 0 && resolve.emitted_result[5] == 1);
    assert(resolve.emitted_result[6] == 0 && resolve.emitted_result[7] == 1);
    assert(resolve.emitted_result[8] == 0 && resolve.emitted_result[9] == 2);
    assert(memcmp(resolve.emitted_result + 10, "c3", 2) == 0);
    assert(resolve.emitted_result[12] == 0 && resolve.emitted_result[13] == 0 &&
           resolve.emitted_result[14] == 0 &&
           resolve.emitted_result[15] == PRDB_SDK_VALUE_TYPE_TEXT);
    assert(resolve.emitted_result[18] == PRDB_SDK_VALUE_STATE_VALUE);
    assert(resolve.emitted_result[19] == 0 && resolve.emitted_result[20] == 0 &&
           resolve.emitted_result[21] == 0 && resolve.emitted_result[22] == 1);
    assert(resolve.emitted_result[23] == 'x');
    resolve.emitted_result_len = 0;
    resolve.statement_failures = 0;
    resolve.result_write_calls = 0;
    resolve.fail_result_write = 1;
    int32_t result_failure_status = prdb_sdk_optimizer_execute_select_ast_to_result_v1(
               &host_v4, &statement, (const uint8_t *)"public", 6, 2,
               sizeof(batch_response), request, sizeof(request),
               optimizer_response, sizeof(optimizer_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, result_record_buffer,
               sizeof(result_record_buffer), &total_rows);
    assert(result_failure_status ==
           PRDB_SDK_OPTIMIZER_PLAN_ROW_CALLBACK_ERROR);
    assert(resolve.result_write_calls == 1);
    assert(resolve.statement_failures == 1);
    resolve.fail_result_write = 0;
    resolve.statement_failures = 0;

    uint8_t sql_with_filter[] = "SELECT c3 FROM items WHERE c3 = $1";
    assert(prdb_sdk_sql_parse_select_v2(
               sql_with_filter, sizeof(sql_with_filter) - 1, &statement,
               sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    uint32_t optimizer_calls = resolve.optimizer_calls;
    assert(prdb_sdk_optimizer_execute_select_ast_v1(
               &host_v4, &statement, (const uint8_t *)"public", 6, 2,
               sizeof(batch_response), request, sizeof(request),
               optimizer_response, sizeof(optimizer_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_UNSUPPORTED);
    assert(resolve.optimizer_calls == optimizer_calls &&
           resolve.statement_failures == 0);

    assert(prdb_sdk_sql_parse_select_v2(
               sql_filtered, sizeof(sql_filtered) - 1, &statement,
               sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    resolve.optimizer_response_len = build_optimizer_response(
        resolve.optimizer_response, filtered_plan, filtered_plan_len);
    resolve.optimizer_calls = 0;
    resolve.calls = 0;
    resolve.event_count = 0;
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_execute_select_ast_with_parameters_v1(
               &host_v4, &statement, filter_parameters, 1,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.optimizer_calls == 1 && resolve.calls == 7 &&
           total_rows == 1 && projected_rows_seen == 1 &&
           resolve.statement_failures == 0);
    resolve.emitted_result_len = 0;
    resolve.result_write_calls = 0;
    assert(prdb_sdk_optimizer_execute_select_ast_with_parameters_to_result_v1(
               &host_v4, &statement, filter_parameters, 1,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, result_record_buffer,
               sizeof(result_record_buffer), &total_rows) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(total_rows == 1 && resolve.result_write_calls == 1 &&
           resolve.emitted_result_len == 27);
    assert(memcmp(resolve.emitted_result, "SQLR", 4) == 0);
    assert(resolve.emitted_result[8] == 0 && resolve.emitted_result[9] == 2 &&
           memcmp(resolve.emitted_result + 10, "c0", 2) == 0);
    assert(resolve.emitted_result[15] == PRDB_SDK_VALUE_TYPE_INTEGER &&
           resolve.emitted_result[23] == 0 && resolve.emitted_result[24] == 0 &&
           resolve.emitted_result[25] == 0 && resolve.emitted_result[26] == 1);

    uint8_t boolean_filtered_plan[512];
    uint32_t boolean_predicate_offset;
    uint32_t boolean_filtered_plan_len = build_boolean_filter_plan(
        boolean_filtered_plan, &boolean_predicate_offset);
    (void)boolean_predicate_offset;
    uint8_t sql_boolean_filtered[] =
        "SELECT c0 FROM items WHERE c0 = $1 AND c0 = $1 LIMIT 1";
    assert(prdb_sdk_sql_parse_select_v2(
               sql_boolean_filtered, sizeof(sql_boolean_filtered) - 1,
               &statement, sizeof(statement), NULL) == PRDB_SDK_SQL_OK);
    resolve.optimizer_response_len = build_optimizer_response(
        resolve.optimizer_response, boolean_filtered_plan,
        boolean_filtered_plan_len);
    resolve.optimizer_calls = 0;
    resolve.calls = 0;
    resolve.event_count = 0;
    projected_rows_seen = 0;
    assert(prdb_sdk_optimizer_execute_select_ast_with_parameters_v1(
               &host_v4, &statement, filter_parameters, 1,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.optimizer_calls == 1 && total_rows == 1 &&
           projected_rows_seen == 1 && resolve.statement_failures == 0);

    assert(prdb_sdk_sql_parse_select_v2(
               sql, sizeof(sql) - 1, &statement, sizeof(statement), NULL) ==
           PRDB_SDK_SQL_OK);
    resolve.optimizer_response[2] = PRDB_SDK_OPTIMIZER_STATUS_INTERNAL;
    resolve.optimizer_response_len = 3;
    assert(prdb_sdk_optimizer_execute_select_ast_v1(
               &host_v4, &statement, (const uint8_t *)"public", 6, 2,
               sizeof(batch_response), request, sizeof(request),
               optimizer_response, sizeof(optimizer_response), response,
               sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, fail_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_CALLBACK_ERROR);
    assert(resolve.statement_failures == 1);
    resolve.cancellation_response[2] = 1;
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_execute_unfiltered_plan_v1(
               &host, limit_one_plan, payload_len, 2, sizeof(batch_response),
               response, sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, source_cells, 4,
               projected_values, 1, on_projected_row, NULL,
               &total_rows) == PRDB_SDK_OPTIMIZER_PLAN_CANCELLED);
    assert(total_rows == 0 && resolve.calls == 5);
    resolve.cancellation_response[2] = 0;

    memcpy(malformed, payload, payload_len);
    malformed[projected_id_offset] = 9;
    assert(prdb_sdk_optimizer_decode_plan_v1(
               malformed, payload_len, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    memcpy(malformed, payload, payload_len);
    malformed[project_node_offset] = 2;
    assert(prdb_sdk_optimizer_decode_plan_v1(
               malformed, payload_len, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_UNSUPPORTED);

    memcpy(malformed, payload, payload_len);
    malformed[payload_len] = 0;
    assert(prdb_sdk_optimizer_decode_plan_v1(
               malformed, payload_len + 1, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    payload_len = build_filter_plan(payload, &predicate_offset);
    assert(prdb_sdk_optimizer_decode_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_UNSUPPORTED);
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(filter.present == 1 && filter.prefix == payload + predicate_offset);
    assert(filter.prefix_len == 28);
    assert(decoded.has_limit == 1 && decoded.limit == 1);

    payload_len = build_delete_plan(payload, &predicate_offset);
    assert(prdb_sdk_optimizer_decode_delete_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(decoded.plan_kind == 2 && decoded.scan_column_count == 0 &&
           decoded.projection_count == 0 && decoded.has_limit == 0);
    assert(filter.present == 1 && filter.prefix == payload + predicate_offset &&
           filter.prefix_len == 2);
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    payload[payload_len - 1] = 4; /* SELECT Project instead of Delete */
    assert(prdb_sdk_optimizer_decode_delete_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    payload[payload_len - 1] = 5;
    payload[predicate_offset + 1] = 2; /* malformed Boolean */
    assert(prdb_sdk_optimizer_decode_delete_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    uint8_t update_plan[512];
    uint32_t update_plan_len = build_update_plan(update_plan);
    assert(prdb_sdk_optimizer_decode_update_plan_v1(
               update_plan, update_plan_len, &update_decoded,
               sizeof(update_decoded)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(update_decoded.plan.plan_kind == 3 &&
           update_decoded.plan.scan_column_count == 1 &&
           update_decoded.filter.present == 1 &&
           update_decoded.assignment_count == 1);
    assert(update_decoded.assignments[0].column.ordinal == 0 &&
           update_decoded.assignments[0].column.type_code == 3 &&
           update_decoded.assignments[0].value_kind ==
               PRDB_OPTIMIZER_UPDATE_PARAMETER_VALUE &&
           update_decoded.assignments[0].parameter_index == 2);

    uint32_t join_projection_side_offset;
    uint32_t join_right_key_type_offset;
    uint32_t join_payload_len = build_join_plan(
        join_payload, &join_projection_side_offset,
        &join_right_key_type_offset);
    assert(prdb_sdk_optimizer_decode_join_plan_v1(
               join_payload, join_payload_len, &join_decoded,
               sizeof(join_decoded)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(join_decoded.left_relation.relation_name_len == 5 &&
           memcmp(join_decoded.left_relation.relation_name, "lefts", 5) == 0);
    assert(join_decoded.right_relation.relation_name_len == 6 &&
           memcmp(join_decoded.right_relation.relation_name, "rights", 6) == 0);
    assert(join_decoded.left_relation.scan_column_count == 3 &&
           join_decoded.right_relation.scan_column_count == 3 &&
           join_decoded.projection_count == 2 &&
           join_decoded.projections[0].relation_side == 0 &&
           join_decoded.projections[1].relation_side == 1 &&
           join_decoded.join_key_count == 2 && join_decoded.has_limit == 1 &&
           join_decoded.limit == 5);
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_validate_executor_join_relations_v1(
               &host, join_payload, join_payload_len, response,
               sizeof(response), &response_len) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 2);
    resolve.calls = 0;
    resolve.fail_join_identity = 1;
    assert(prdb_sdk_optimizer_validate_executor_join_relations_v1(
               &host, join_payload, join_payload_len, response,
               sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    assert(resolve.calls == 2);
    resolve.fail_join_identity = 0;
    memcpy(join_malformed, join_payload, join_payload_len);
    join_malformed[join_projection_side_offset] = 2;
    assert(prdb_sdk_optimizer_decode_join_plan_v1(
               join_malformed, join_payload_len, &join_decoded,
               sizeof(join_decoded)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    memcpy(join_malformed, join_payload, join_payload_len);
    join_malformed[join_right_key_type_offset] = 6;
    assert(prdb_sdk_optimizer_decode_join_plan_v1(
               join_malformed, join_payload_len, &join_decoded,
               sizeof(join_decoded)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    memcpy(join_malformed, join_payload, join_payload_len);
    join_malformed[join_payload_len] = 0;
    assert(prdb_sdk_optimizer_decode_join_plan_v1(
               join_malformed, join_payload_len + 1, &join_decoded,
               sizeof(join_decoded)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    assert(prdb_sdk_optimizer_validate_executor_relation_v1(
               &host, update_plan, update_plan_len, response,
               sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(prdb_sdk_optimizer_decode_update_plan_v1(
               update_plan, update_plan_len - 1, &update_decoded,
               sizeof(update_decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    memcpy(malformed, update_plan, update_plan_len);
    malformed[update_plan_len - 5] = 9; /* unsupported assignment kind */
    assert(prdb_sdk_optimizer_decode_update_plan_v1(
               malformed, update_plan_len, &update_decoded,
               sizeof(update_decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    memcpy(malformed, update_plan, update_plan_len);
    malformed[update_plan_len - 1] = 1; /* NULL must have zero parameter index */
    malformed[update_plan_len - 5] = PRDB_OPTIMIZER_UPDATE_NULL_VALUE;
    assert(prdb_sdk_optimizer_decode_update_plan_v1(
               malformed, update_plan_len, &update_decoded,
               sizeof(update_decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);
    assert(prdb_sdk_optimizer_decode_update_plan_v1(
               update_plan, update_plan_len,
               (prdb_sdk_optimizer_update_plan_v1 *)update_plan,
               sizeof(update_decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_ARGUMENT);

    uint8_t update_sql[] = "UPDATE items SET c0 = $2 WHERE c0 = $1";
    uint8_t update_parameter_bytes[2][4] = {{0, 0, 0, 1}, {0, 0, 0, 2}};
    prdb_sdk_value_v1 update_parameters[2] = {
        {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
         PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
         update_parameter_bytes[0], sizeof(update_parameter_bytes[0])},
        {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
         PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
         update_parameter_bytes[1], sizeof(update_parameter_bytes[1])}};
    assert(prdb_sdk_sql_parse_update_v1(
               update_sql, sizeof(update_sql) - 1, &update_statement,
               sizeof(update_statement), NULL) == PRDB_SDK_SQL_OK);
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_validate_update_plan_with_parameters_v1(
               &host, &update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, update_plan, update_plan_len,
               response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 1);
    update_statement.assignments[0].parameter_index = 1;
    assert(prdb_sdk_optimizer_validate_update_plan_with_parameters_v1(
               &host, &update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, update_plan, update_plan_len,
               response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    update_statement.assignments[0].parameter_index = 2;
    update_parameter_bytes[1][0] = 0;
    update_parameters[1].type_code = PRDB_SDK_VALUE_TYPE_BOOLEAN;
    assert(prdb_sdk_optimizer_validate_update_plan_with_parameters_v1(
               &host, &update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, update_plan, update_plan_len,
               response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    update_parameters[1].type_code = PRDB_SDK_VALUE_TYPE_INTEGER;
    update_parameter_bytes[1][0] = 0;

    uint8_t delete_plan[512];
    uint32_t delete_plan_len = build_delete_filter_plan(
        delete_plan, &predicate_offset);
    uint8_t delete_sql[] = "DELETE FROM items WHERE c0 = $1";
    uint8_t delete_parameter_bytes[4] = {0, 0, 0, 1};
    prdb_sdk_value_v1 delete_parameters[1] = {{
        sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
        delete_parameter_bytes, sizeof(delete_parameter_bytes)}};
    assert(prdb_sdk_sql_parse_delete_v1(
               delete_sql, sizeof(delete_sql) - 1, &delete_statement,
               sizeof(delete_statement), NULL) == PRDB_SDK_SQL_OK);
    assert(prdb_sdk_optimizer_decode_delete_plan_v1(
               delete_plan, delete_plan_len, &decoded, sizeof(decoded),
               &filter, sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    resolve.calls = 0;
    int32_t delete_validation_status =
        prdb_sdk_optimizer_validate_delete_plan_with_parameters_v1(
               &host, &delete_statement, delete_parameters, 1,
               (const uint8_t *)"public", 6, delete_plan, delete_plan_len,
               response, sizeof(response), &response_len);
    assert(delete_validation_status == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 1);
    assert(prdb_sdk_optimizer_acquire_plan_relation_v1(
               &host, delete_plan, delete_plan_len, response, sizeof(response),
               &response_len, &acquired_relation, sizeof(acquired_relation),
               acquired_columns, 4) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(acquired_relation.name_len == 5 &&
           memcmp(acquired_relation.name, "items", 5) == 0);
    delete_parameter_bytes[3] = 2;
    assert(prdb_sdk_optimizer_validate_delete_plan_with_parameters_v1(
               &host, &delete_statement, delete_parameters, 1,
               (const uint8_t *)"public", 6, delete_plan, delete_plan_len,
               response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    delete_parameter_bytes[3] = 1;
    uint8_t wrong_delete_sql[] = "DELETE FROM items WHERE c1 = $1";
    assert(prdb_sdk_sql_parse_delete_v1(
               wrong_delete_sql, sizeof(wrong_delete_sql) - 1,
               &delete_statement, sizeof(delete_statement), NULL) ==
           PRDB_SDK_SQL_OK);
    assert(prdb_sdk_optimizer_validate_delete_plan_with_parameters_v1(
               &host, &delete_statement, delete_parameters, 1,
               (const uint8_t *)"public", 6, delete_plan, delete_plan_len,
               response, sizeof(response), &response_len) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);

    payload_len = build_filter_plan(payload, &predicate_offset);
    memcpy(malformed, payload, payload_len);
    malformed[predicate_offset] = 0x10; /* INTEGER is not a Boolean root. */
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               malformed, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    memcpy(malformed, payload, payload_len);
    malformed[predicate_offset + 19] = 0x01; /* Boolean versus INTEGER. */
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               malformed, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    memcpy(malformed, payload, payload_len);
    malformed[predicate_offset] = 0x03;
    malformed[predicate_offset + 1] = 0;
    malformed[predicate_offset + 2] = 1;
    malformed[predicate_offset + 3] = 0xff;
    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               malformed, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter)) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_PAYLOAD);

    assert(prdb_sdk_optimizer_decode_plan_with_filter_v1(
               payload, payload_len, &decoded, sizeof(decoded), &filter,
               sizeof(filter) - 1) == PRDB_SDK_OPTIMIZER_PLAN_INVALID_ARGUMENT);

    payload_len = build_plan(payload, &projected_id_offset,
                             &project_node_offset);
    assert(prdb_sdk_optimizer_decode_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded) - 1) ==
           PRDB_SDK_OPTIMIZER_PLAN_INVALID_ARGUMENT);
    assert(prdb_sdk_optimizer_decode_plan_v1(
               payload, payload_len, &decoded, sizeof(decoded)) ==
           PRDB_SDK_OPTIMIZER_PLAN_OK);
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_acquire_plan_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len, &acquired_relation, sizeof(acquired_relation),
               acquired_columns, 4) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 2);
    assert(acquired_relation.relation_id[0] == 2);
    assert(acquired_relation.executor_handle[0] == 6);
    assert(acquired_relation.column_count == 4);
    assert(acquired_columns[3].ordinal == 3 &&
           acquired_columns[3].type_code == 6);
    uint8_t cursor[16] = {0};
    uint32_t scan_opened = 0;
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_open_plan_scan_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len, malformed, sizeof(malformed),
               &scan_response_len,
               &acquired_relation, sizeof(acquired_relation), acquired_columns,
               4, cursor, &scan_opened) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 3 && scan_opened == 1 && cursor[0] == 7);
    uint8_t zero_limit_plan[sizeof(payload)];
    memcpy(zero_limit_plan, payload, payload_len);
    zero_limit_plan[payload_len - 1] = 0;
    resolve.calls = 0;
    assert(prdb_sdk_optimizer_open_plan_scan_v1(
               &host, zero_limit_plan, payload_len, response, sizeof(response),
               &response_len, malformed, sizeof(malformed),
               &scan_response_len,
               &acquired_relation, sizeof(acquired_relation), acquired_columns,
               4, cursor, &scan_opened) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(resolve.calls == 2 && scan_opened == 0 && cursor[0] == 0);
    resolve.executor_response[3] = 9;
    assert(prdb_sdk_optimizer_acquire_plan_relation_v1(
               &host, payload, payload_len, response, sizeof(response),
               &response_len, &acquired_relation, sizeof(acquired_relation),
               acquired_columns, 4) ==
           PRDB_SDK_OPTIMIZER_PLAN_IDENTITY_MISMATCH);
    resolve.executor_response[3] = 2;

    uint8_t execute_delete_sql[] = "DELETE FROM items WHERE c0 = $1";
    uint8_t execute_delete_parameter_bytes[4] = {0, 0, 0, 1};
    prdb_sdk_value_v1 execute_delete_parameters[1] = {{
        sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
        execute_delete_parameter_bytes,
        sizeof(execute_delete_parameter_bytes)}};
    uint8_t execute_delete_plan[512];
    uint32_t execute_delete_plan_len = build_delete_filter_plan(
        execute_delete_plan, &predicate_offset);
    uint32_t deleted_count = 0;
    assert(prdb_sdk_sql_parse_delete_v1(
               execute_delete_sql, sizeof(execute_delete_sql) - 1,
               &delete_statement,
               sizeof(delete_statement), NULL) == PRDB_SDK_SQL_OK);
    resolve.optimizer_response_len = build_optimizer_response(
        resolve.optimizer_response, execute_delete_plan,
        execute_delete_plan_len);
    resolve.batch_response_len = build_row_batch(resolve.batch_response);
    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_delete_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    host_v5.v4 = host_v4;
    host_v5.optimizer_request = optimizer_request_v5;
    assert(prdb_sdk_optimizer_encode_delete_v1(
               &delete_statement, execute_delete_parameters, 1,
               (const uint8_t *)"public", 6, request, sizeof(request),
               &response_len) == PRDB_SDK_OPTIMIZER_ENCODE_OK);
    assert(prdb_sdk_optimizer_execute_delete_ast_v1(
               &host_v5, &delete_statement, execute_delete_parameters, 1,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response), request,
               sizeof(request), optimizer_response, sizeof(optimizer_response),
               response, sizeof(response), scan_response, sizeof(scan_response),
               batch_response, sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles,
               PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
               &deleted_count) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(deleted_count == 1 && resolve.optimizer_calls == 1 &&
           resolve.stage_delete_calls == 1 && resolve.statement_failures == 0);
    assert(resolve.calls == 7);

    uint8_t execute_update_sql[] =
        "UPDATE items SET c0 = $2 WHERE c0 = $1";
    static prdb_sdk_update_v1 execute_update_statement;
    assert(prdb_sdk_sql_parse_update_v1(
               execute_update_sql, sizeof(execute_update_sql) - 1,
               &execute_update_statement, sizeof(execute_update_statement),
               NULL) == PRDB_SDK_SQL_OK);
    uint8_t execute_update_plan[512];
    uint32_t execute_update_plan_len = build_update_plan(execute_update_plan);
    resolve.optimizer_response_len = build_optimizer_response(
        resolve.optimizer_response, execute_update_plan,
        execute_update_plan_len);
    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_update_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    uint32_t updated_count = 0;
    assert(prdb_sdk_optimizer_execute_update_ast_v1(
               &host_v5, &execute_update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles,
               PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
               &updated_count) == PRDB_SDK_OPTIMIZER_PLAN_OK);
    assert(updated_count == 1 && resolve.optimizer_calls == 1 &&
           resolve.stage_update_calls == 1 && resolve.statement_failures == 0);
    assert(resolve.calls == 7);

    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_update_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    resolve.cancellation_response[2] = 1;
    updated_count = 99;
    assert(prdb_sdk_optimizer_execute_update_ast_v1(
               &host_v5, &execute_update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles,
               PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
               &updated_count) == PRDB_SDK_OPTIMIZER_PLAN_CANCELLED);
    assert(updated_count == 0 && resolve.optimizer_calls == 1 &&
           resolve.stage_update_calls == 0 && resolve.statement_failures == 1);
    assert(resolve.calls == 6);
    assert(resolve.events[6] == PRDB_SDK_EXECUTOR_OP_CLOSE_CURSOR);
    resolve.cancellation_response[2] = 0;

    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_update_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    resolve.fail_stage_update = 1;
    updated_count = 99;
    assert(prdb_sdk_optimizer_execute_update_ast_v1(
               &host_v5, &execute_update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles,
               PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
               &updated_count) == PRDB_SDK_OPTIMIZER_PLAN_CALLBACK_ERROR);
    assert(updated_count == 0 && resolve.optimizer_calls == 1 &&
           resolve.stage_update_calls == 1 && resolve.statement_failures == 1);
    assert(resolve.calls == 7);
    assert(resolve.events[6] == PRDB_SDK_EXECUTOR_OP_NEXT_BATCH);
    assert(resolve.events[7] == PRDB_SDK_EXECUTOR_OP_STAGE_UPDATE);
    resolve.fail_stage_update = 0;

    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_update_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    resolve.fail_optimizer_request = 1;
    updated_count = 99;
    assert(prdb_sdk_optimizer_execute_update_ast_v1(
               &host_v5, &execute_update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles,
               PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
               &updated_count) == PRDB_SDK_OPTIMIZER_PLAN_CALLBACK_ERROR);
    assert(updated_count == 0 && resolve.optimizer_calls == 1 &&
           resolve.calls == 0 && resolve.stage_update_calls == 0 &&
           resolve.statement_failures == 1);
    assert(resolve.event_count == 1 && resolve.events[0] == UINT16_C(0xffff));
    resolve.fail_optimizer_request = 0;

    resolve.batch_response_len = build_two_row_batch(resolve.batch_response);
    resolve.calls = 0;
    resolve.optimizer_calls = 0;
    resolve.stage_update_calls = 0;
    resolve.statement_failures = 0;
    resolve.event_count = 0;
    updated_count = 99;
    assert(prdb_sdk_optimizer_execute_update_ast_v1(
               &host_v5, &execute_update_statement, update_parameters, 2,
               (const uint8_t *)"public", 6, 2, sizeof(batch_response),
               request, sizeof(request), optimizer_response,
               sizeof(optimizer_response), response, sizeof(response),
               scan_response, sizeof(scan_response), batch_response,
               sizeof(batch_response), &acquired_relation,
               sizeof(acquired_relation), acquired_columns, 4, &delete_binding,
               sizeof(delete_binding), delete_cell_scratch, 4,
               delete_target_handles, 1, &updated_count) ==
           PRDB_SDK_OPTIMIZER_PLAN_REQUEST_LIMIT);
    assert(updated_count == 0 && resolve.optimizer_calls == 1 &&
           resolve.stage_update_calls == 0 && resolve.statement_failures == 1);
    return 0;
}
