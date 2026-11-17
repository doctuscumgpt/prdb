#include "prdb_sdk_sql_dispatch_v1.h"

#include <stdio.h>
#include <string.h>

typedef struct dispatch_test_context {
    uint32_t planner_calls;
    uint32_t failure_calls;
} dispatch_test_context;

static int32_t fail_planner_request(
    void *opaque, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    dispatch_test_context *context = (dispatch_test_context *)opaque;
    (void)operation;
    (void)request;
    (void)request_len;
    (void)response;
    (void)response_capacity;
    if (context == NULL || response_len == NULL) return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    ++context->planner_calls;
    *response_len = 0;
    return PRDB_QUERY_CALLBACK_IO_ERROR;
}

static int32_t unused_executor_request(
    void *opaque, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    (void)opaque;
    (void)operation;
    (void)request;
    (void)request_len;
    (void)response;
    (void)response_capacity;
    if (response_len == NULL) return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    *response_len = 0;
    return PRDB_QUERY_CALLBACK_IO_ERROR;
}

static int32_t discard_result(void *opaque, const uint8_t *bytes,
                              uint64_t bytes_len) {
    (void)opaque;
    (void)bytes;
    (void)bytes_len;
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
