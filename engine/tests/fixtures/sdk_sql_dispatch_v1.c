#include "prdb_sdk_sql_dispatch_v1.h"

#include <stdio.h>
#include <string.h>

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
