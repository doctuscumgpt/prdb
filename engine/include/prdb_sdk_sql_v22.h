#ifndef PRDB_SDK_SQL_V22_H
#define PRDB_SDK_SQL_V22_H

#include "prdb_sdk_sql_v21.h"

#define PRDB_SDK_SQL_ABI_V22_VERSION UINT32_C(22)

/* Bit i marks a HAVING comparison whose left operand is hidden COUNT(*). */
typedef struct prdb_sdk_select_v22 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_v21 base;
    uint64_t having_count_mask;
} prdb_sdk_select_v22;

typedef struct prdb_sdk_select_binding_v22 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_binding_v21 base;
    uint64_t having_count_mask;
} prdb_sdk_select_binding_v22;

int32_t prdb_sdk_sql_parse_select_v22(const uint8_t *sql, uint64_t sql_len,
                                      prdb_sdk_select_v22 *out,
                                      uint32_t out_capacity,
                                      uint32_t *error_offset_out);

int32_t prdb_sdk_sql_bind_select_v22(
    const prdb_sdk_select_v22 *statement, const uint8_t *relation_name,
    uint32_t relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *columns, uint32_t column_count,
    prdb_sdk_select_binding_v22 *out, uint32_t out_capacity);

#endif /* PRDB_SDK_SQL_V22_H */
