#include "prdb_sdk_sql_dispatch_v1.h"
#include "prdb_sdk_executor_v1.h"
#include "prdb_sdk_planner_v1.h"

#include <stdio.h>
#include <string.h>

typedef struct dispatch_test_context {
    uint32_t planner_calls;
    uint32_t failure_calls;
    uint32_t group_success;
    uint32_t next_calls;
    uint32_t result_calls;
    uint32_t result_len;
    uint8_t planner_reply[256];
    uint32_t planner_reply_len;
    uint8_t executor_reply[256];
    uint32_t executor_reply_len;
    uint8_t batch_reply[512];
    uint32_t batch_reply_len;
    uint8_t result_bytes[512];
} dispatch_test_context;

static void put_u16(uint8_t *bytes, uint32_t *offset, uint16_t value) {
    bytes[(*offset)++] = (uint8_t)(value >> 8);
    bytes[(*offset)++] = (uint8_t)value;
}

static void put_u32_at(uint8_t *bytes, uint32_t *offset, uint32_t value) {
    bytes[(*offset)++] = (uint8_t)(value >> 24);
    bytes[(*offset)++] = (uint8_t)(value >> 16);
    bytes[(*offset)++] = (uint8_t)(value >> 8);
    bytes[(*offset)++] = (uint8_t)value;
}

static void put_name(uint8_t *bytes, uint32_t *offset, const char *name) {
    uint16_t length = (uint16_t)strlen(name);
    put_u16(bytes, offset, length);
    memcpy(bytes + *offset, name, length);
    *offset += length;
}

static uint32_t make_integer_relation(uint8_t *bytes, uint8_t handle_kind,
                                      uint8_t handle_fill) {
    uint32_t offset = 0;
    put_u16(bytes, &offset, 1);
    bytes[offset++] = 0;
    memset(bytes + offset, 0x11, 16); offset += 16;
    bytes[offset++] = handle_kind;
    memset(bytes + offset, handle_fill, 15); offset += 15;
    put_name(bytes, &offset, "widgets");
    put_u16(bytes, &offset, 2);
    for (uint32_t index = 0; index < 2; ++index) {
        memset(bytes + offset, (int)(0x33 + index), 16); offset += 16;
        put_u32_at(bytes, &offset, index);
        bytes[offset++] = 1;
        bytes[offset++] = PRDB_SDK_VALUE_TYPE_INTEGER;
        bytes[offset++] = 0;
        bytes[offset++] = 0;
        bytes[offset++] = 0;
        bytes[offset++] = 0;
        put_name(bytes, &offset, index == 0 ? "id" : "score");
    }
    return offset;
}

static uint32_t make_group_rows(uint8_t *bytes) {
    uint32_t offset = 0;
    put_u16(bytes, &offset, 1);
    bytes[offset++] = 1;
    put_u16(bytes, &offset, 2);
    for (uint32_t row = 0; row < 2; ++row) {
        bytes[offset++] = 8;
        memset(bytes + offset, (int)(row + 1), 15); offset += 15;
        put_u16(bytes, &offset, 2);
        for (uint32_t cell = 0; cell < 2; ++cell) {
            bytes[offset++] = PRDB_SDK_VALUE_STATE_VALUE;
            bytes[offset++] = PRDB_SDK_VALUE_TYPE_INTEGER;
            put_u32_at(bytes, &offset, 4);
            put_u32_at(bytes, &offset, cell == 0 ? 1 : 3);
        }
    }
    return offset;
}

static int32_t fail_planner_request(
    void *opaque, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    dispatch_test_context *context = (dispatch_test_context *)opaque;
    (void)operation;
    (void)request;
    (void)request_len;
    if (context == NULL || response_len == NULL) return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    ++context->planner_calls;
    if (context->group_success &&
        operation == PRDB_SDK_PLANNER_OP_DESCRIBE_RELATION) {
        if (context->planner_reply_len > response_capacity)
            return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
        memcpy(response, context->planner_reply, context->planner_reply_len);
        *response_len = context->planner_reply_len;
        return PRDB_QUERY_CALLBACK_OK;
    }
    *response_len = 0;
    return PRDB_QUERY_CALLBACK_IO_ERROR;
}

static int32_t unused_executor_request(
    void *opaque, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    dispatch_test_context *context = (dispatch_test_context *)opaque;
    if (context == NULL || response_len == NULL)
        return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    if (!context->group_success) {
        *response_len = 0;
        return PRDB_QUERY_CALLBACK_IO_ERROR;
    }
    if (operation == PRDB_SDK_EXECUTOR_OP_RESOLVE_RELATION) {
        if (context->executor_reply_len > response_capacity)
            return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
        memcpy(response, context->executor_reply, context->executor_reply_len);
        *response_len = context->executor_reply_len;
    } else if (operation == PRDB_SDK_EXECUTOR_OP_CHECK_CANCELLATION) {
        static const uint8_t reply[] = {0, 1, 0};
        if (request_len != 0 || sizeof(reply) > response_capacity)
            return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_OPEN_SCAN) {
        uint8_t reply[18] = {0, 1, 7};
        memset(reply + 3, 0x44, 15);
        if (request_len == 0 || sizeof(reply) > response_capacity)
            return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_NEXT_BATCH) {
        if (context->next_calls++ == 0) {
            if (context->batch_reply_len > response_capacity)
                return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
            memcpy(response, context->batch_reply, context->batch_reply_len);
            *response_len = context->batch_reply_len;
        } else {
            static const uint8_t finished[] = {0, 1, 1, 0, 0};
            if (sizeof(finished) > response_capacity)
                return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
            memcpy(response, finished, sizeof(finished));
            *response_len = sizeof(finished);
        }
    } else if (operation == PRDB_SDK_EXECUTOR_OP_CLOSE_CURSOR) {
        static const uint8_t reply[] = {0, 1, 0};
        if (sizeof(reply) > response_capacity)
            return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else {
        return PRDB_QUERY_CALLBACK_IO_ERROR;
    }
    (void)request;
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t discard_result(void *opaque, const uint8_t *bytes,
                              uint64_t bytes_len) {
    dispatch_test_context *context = (dispatch_test_context *)opaque;
    if (context != NULL && context->group_success) {
        if (bytes_len > sizeof(context->result_bytes) - context->result_len)
            return PRDB_QUERY_CALLBACK_RESPONSE_TOO_SMALL;
        memcpy(context->result_bytes + context->result_len, bytes,
               (size_t)bytes_len);
        context->result_len += (uint32_t)bytes_len;
        ++context->result_calls;
    }
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t count_statement_failure(void *opaque, uint32_t reason) {
    dispatch_test_context *context = (dispatch_test_context *)opaque;
    (void)reason;
    if (context == NULL) return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    ++context->failure_calls;
    return PRDB_QUERY_CALLBACK_OK;
}

static int test_statement_dispatch_routes_supported_dml(void) {
    static const char *statements[] = {
        "INSERT INTO t VALUES ($1)",
        "UPDATE t SET x = NULL WHERE id = $1",
        "DELETE FROM t WHERE id = $1",
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND SUM(score) >= 4.50",
    };
    dispatch_test_context context = {0};
    prdb_host_api_v3 host = {0};
    host.struct_size = sizeof(host);
    host.abi_version = PRDB_QUERY_ABI_V3_VERSION;
    host.context = &context;
    host.executor_request = unused_executor_request;
    host.planner_request = fail_planner_request;
    host.write_result = discard_result;
    host.statement_failure = count_statement_failure;
    for (uint32_t index = 0; index < sizeof(statements) / sizeof(statements[0]);
         ++index) {
        const char *sql = statements[index];
        int32_t status = prdb_sdk_sql_run_statement_v1(
            &host, (const uint8_t *)"public", 6, (const uint8_t *)sql,
            strlen(sql), NULL, 0);
        if (status != PRDB_SDK_SQL_DISPATCH_EXECUTION_ERROR ||
            context.planner_calls != index + 1 ||
            context.failure_calls != index + 1) {
            return 1;
        }
    }
    return 0;
}

static uint32_t read_u32(const uint8_t *bytes) {
    return ((uint32_t)bytes[0] << 24) | ((uint32_t)bytes[1] << 16) |
           ((uint32_t)bytes[2] << 8) | bytes[3];
}

static int test_dispatch_grouped_select_emits_sqlr(void) {
    static const uint8_t sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND SUM(score) >= 4";
    dispatch_test_context context = {0};
    prdb_host_api_v3 host = {0};
    uint32_t offset = 0;
    uint16_t name_len;

    context.group_success = 1;
    context.planner_reply_len = make_integer_relation(
        context.planner_reply, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x22);
    context.executor_reply_len = make_integer_relation(
        context.executor_reply, 6, 0x22);
    context.batch_reply_len = make_group_rows(context.batch_reply);
    host.struct_size = sizeof(host);
    host.abi_version = PRDB_QUERY_ABI_V3_VERSION;
    host.context = &context;
    host.executor_request = unused_executor_request;
    host.planner_request = fail_planner_request;
    host.write_result = discard_result;
    host.statement_failure = count_statement_failure;
    if (prdb_sdk_sql_run_statement_v1(
            &host, (const uint8_t *)"app", 3, sql, sizeof(sql) - 1,
            NULL, 0) != PRDB_SDK_SQL_DISPATCH_OK ||
        context.planner_calls != 1 || context.failure_calls != 0 ||
        context.result_calls != 1 || context.result_len < 8 ||
        memcmp(context.result_bytes, "SQLR", 4) != 0) return 1;
    offset = 4;
    if (context.result_bytes[offset++] != 0 ||
        context.result_bytes[offset++] != 1 ||
        context.result_bytes[offset++] != 0 ||
        context.result_bytes[offset++] != 3) return 1;
    name_len = (uint16_t)(((uint16_t)context.result_bytes[offset] << 8) |
                         context.result_bytes[offset + 1]);
    offset += 2;
    if (name_len != 2 || memcmp(context.result_bytes + offset, "id", 2) != 0)
        return 1;
    offset += name_len;
    if (read_u32(context.result_bytes + offset) !=
            PRDB_SDK_VALUE_TYPE_INTEGER) return 1;
    offset += 6;
    if (context.result_bytes[offset++] != PRDB_SDK_VALUE_STATE_VALUE ||
        read_u32(context.result_bytes + offset) != 4) return 1;
    offset += 4;
    if (read_u32(context.result_bytes + offset) != 1) return 1;
    offset += 4;

    name_len = (uint16_t)(((uint16_t)context.result_bytes[offset] << 8) |
                         context.result_bytes[offset + 1]);
    offset += 2;
    if (name_len != 5 || memcmp(context.result_bytes + offset, "score", 5) != 0)
        return 1;
    offset += name_len + 6;
    if (context.result_bytes[offset++] != PRDB_SDK_VALUE_STATE_VALUE ||
        read_u32(context.result_bytes + offset) != 4) return 1;
    offset += 4;
    if (read_u32(context.result_bytes + offset) != 3) return 1;
    offset += 4;

    name_len = (uint16_t)(((uint16_t)context.result_bytes[offset] << 8) |
                         context.result_bytes[offset + 1]);
    offset += 2;
    if (name_len != 3 || memcmp(context.result_bytes + offset, "sum", 3) != 0)
        return 1;
    offset += name_len;
    if (read_u32(context.result_bytes + offset) != PRDB_SDK_VALUE_TYPE_BIGINT)
        return 1;
    offset += 6;
    if (context.result_bytes[offset++] != PRDB_SDK_VALUE_STATE_VALUE ||
        read_u32(context.result_bytes + offset) != 8) return 1;
    offset += 4;
    if (read_u32(context.result_bytes + offset + 4) != 6) return 1;
    offset += 8;
    if (offset != context.result_len) return 1;
    return 0;
}

static int test_dispatch_single_key_grouped_select_emits_sqlr(void) {
    static const uint8_t sql[] =
        "SELECT id, SUM(score) FROM widgets GROUP BY id";
    dispatch_test_context context = {0};
    prdb_host_api_v3 host = {0};
    uint32_t offset = 0;
    uint16_t name_len;

    context.group_success = 1;
    context.planner_reply_len = make_integer_relation(
        context.planner_reply, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x22);
    context.executor_reply_len = make_integer_relation(
        context.executor_reply, 6, 0x22);
    context.batch_reply_len = make_group_rows(context.batch_reply);
    host.struct_size = sizeof(host);
    host.abi_version = PRDB_QUERY_ABI_V3_VERSION;
    host.context = &context;
    host.executor_request = unused_executor_request;
    host.planner_request = fail_planner_request;
    host.write_result = discard_result;
    host.statement_failure = count_statement_failure;
    if (prdb_sdk_sql_run_statement_v1(
            &host, (const uint8_t *)"app", 3, sql, sizeof(sql) - 1,
            NULL, 0) != PRDB_SDK_SQL_DISPATCH_OK ||
        context.planner_calls != 1 || context.failure_calls != 0 ||
        context.result_calls != 1 || context.result_len < 8 ||
        memcmp(context.result_bytes, "SQLR", 4) != 0) return 1;
    offset = 4;
    if (context.result_bytes[offset++] != 0 ||
        context.result_bytes[offset++] != 1 ||
        context.result_bytes[offset++] != 0 ||
        context.result_bytes[offset++] != 2) return 1;
    name_len = (uint16_t)(((uint16_t)context.result_bytes[offset] << 8) |
                         context.result_bytes[offset + 1]);
    offset += 2;
    if (name_len != 2 || memcmp(context.result_bytes + offset, "id", 2) != 0)
        return 1;
    offset += name_len;
    if (read_u32(context.result_bytes + offset) !=
            PRDB_SDK_VALUE_TYPE_INTEGER) return 1;
    offset += 6;
    if (context.result_bytes[offset++] != PRDB_SDK_VALUE_STATE_VALUE ||
        read_u32(context.result_bytes + offset) != 4) return 1;
    offset += 4;
    if (read_u32(context.result_bytes + offset) != 1) return 1;
    offset += 4;
    name_len = (uint16_t)(((uint16_t)context.result_bytes[offset] << 8) |
                         context.result_bytes[offset + 1]);
    offset += 2;
    if (name_len != 3 || memcmp(context.result_bytes + offset, "sum", 3) != 0)
        return 1;
    offset += name_len;
    if (read_u32(context.result_bytes + offset) != PRDB_SDK_VALUE_TYPE_BIGINT)
        return 1;
    offset += 6;
    if (context.result_bytes[offset++] != PRDB_SDK_VALUE_STATE_VALUE ||
        read_u32(context.result_bytes + offset) != 8) return 1;
    offset += 4;
    if (read_u32(context.result_bytes + offset + 4) != 6) return 1;
    offset += 8;
    if (offset != context.result_len) return 1;
    return 0;
}

static void put_u32(uint8_t *out, uint32_t value) {
    out[0] = (uint8_t)(value >> 24);
    out[1] = (uint8_t)(value >> 16);
    out[2] = (uint8_t)(value >> 8);
    out[3] = (uint8_t)value;
}

static int test_typed_values_and_null(void) {
    uint8_t bytes[] = {
        'P','S','P','1', 0,2, 0,0,
        0,0,0,3, 0,0, 0,0, 0,0,0,4, 42,0,0,0,
        0,0,0,5, 8,2, 1,0, 0,0,0,0
    };
    prdb_sdk_value_v1 values[2] = {0};
    prdb_sdk_sql_parameter_metadata_v1 metadata[2] = {0};
    uint32_t count = 99;
    int32_t result = prdb_sdk_sql_decode_parameters_v1(
        bytes, sizeof(bytes), values, 2, metadata, 2, &count);
    if (result != PRDB_SDK_SQL_DISPATCH_OK || count != 2 ||
        values[0].type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
        values[0].data != bytes + 20 || values[0].data_len != 4 ||
        metadata[0].type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
        values[1].state != PRDB_SDK_VALUE_STATE_NULL ||
        values[1].type_code != 0 || values[1].data != NULL ||
        metadata[1].type_code != PRDB_SDK_VALUE_TYPE_DECIMAL ||
        metadata[1].decimal_precision != 8 || metadata[1].decimal_scale != 2) {
        return 1;
    }
    return 0;
}

static int test_rejects_malformed_blocks(void) {
    uint8_t bytes[] = {
        'P','S','P','1', 0,1, 0,0,
        0,0,0,3, 0,0, 0,0, 0,0,0,4, 1,0,0,0
    };
    prdb_sdk_value_v1 values[1] = {0};
    prdb_sdk_sql_parameter_metadata_v1 metadata[1] = {0};
    uint32_t count = 99;
    if (prdb_sdk_sql_decode_parameters_v1(bytes, sizeof(bytes) - 1,
            values, 1, metadata, 1, &count) !=
            PRDB_SDK_SQL_DISPATCH_INVALID_PARAMETERS || count != 0) return 1;
    bytes[15] = 1;
    if (prdb_sdk_sql_decode_parameters_v1(bytes, sizeof(bytes),
            values, 1, metadata, 1, &count) !=
            PRDB_SDK_SQL_DISPATCH_INVALID_PARAMETERS) return 1;
    bytes[15] = 0;
    put_u32(bytes + 8, PRDB_SDK_VALUE_TYPE_DECIMAL);
    bytes[12] = 39;
    if (prdb_sdk_sql_decode_parameters_v1(bytes, sizeof(bytes),
            values, 1, metadata, 1, &count) !=
            PRDB_SDK_SQL_DISPATCH_INVALID_PARAMETERS) return 1;
    return 0;
}

int main(void) {
    if (prdb_sdk_sql_run_statement_v1(NULL, NULL, 0, NULL, 0, NULL, 0) !=
        PRDB_SDK_SQL_DISPATCH_INVALID_ARGUMENT) {
        fputs("interactive statement dispatcher accepted an invalid host\n", stderr);
        return 1;
    }
    if (test_statement_dispatch_routes_supported_dml() != 0) {
        fputs("interactive statement dispatcher did not route supported DML\n", stderr);
        return 1;
    }
    if (test_dispatch_grouped_select_emits_sqlr() != 0) {
        fputs("grouped SELECT dispatch did not emit expected SQLR\n", stderr);
        return 1;
    }
    if (test_dispatch_single_key_grouped_select_emits_sqlr() != 0) {
        fputs("single-key grouped SELECT dispatch did not emit expected SQLR\n", stderr);
        return 1;
    }
    if (test_typed_values_and_null() != 0) {
        fputs("typed PSP1 decode or NULL metadata failed\n", stderr);
        return 1;
    }
    if (test_rejects_malformed_blocks() != 0) {
        fputs("malformed PSP1 block was accepted\n", stderr);
        return 1;
    }
    return 0;
}
