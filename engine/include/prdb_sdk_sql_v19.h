#ifndef PRDB_SDK_SQL_V19_H
#define PRDB_SDK_SQL_V19_H

#include "prdb_sdk_sql_v16.h"

#define PRDB_SDK_SQL_ABI_V19_VERSION UINT32_C(19)

typedef struct prdb_sdk_select_v19 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_v16 base;
    prdb_sdk_sql_span_v1 left_alias;
    prdb_sdk_sql_span_v1 right_alias;
} prdb_sdk_select_v19;

typedef struct prdb_sdk_select_binding_v19 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_binding_v16 base;
} prdb_sdk_select_binding_v19;

/* Parses the bounded v16 INNER JOIN form with optional explicit AS aliases.
 * Qualified projections and join predicates use the aliases when present. */
int32_t prdb_sdk_sql_parse_select_v19(const uint8_t *sql, uint64_t sql_len,
                                      prdb_sdk_select_v19 *out,
                                      uint32_t out_capacity,
                                      uint32_t *error_offset_out);

/* Binds a v19 AST against independently resolved relation shapes. Alias
 * qualifiers are resolved to their underlying relation side before using the
 * frozen v16 type and column checks. */
int32_t prdb_sdk_sql_bind_select_v19(
    const prdb_sdk_select_v19 *statement,
    const uint8_t *left_relation_name, uint32_t left_relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *left_columns,
    uint32_t left_column_count,
    const uint8_t *right_relation_name, uint32_t right_relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *right_columns,
    uint32_t right_column_count, prdb_sdk_select_binding_v19 *out,
    uint32_t out_capacity);

#endif /* PRDB_SDK_SQL_V19_H */
