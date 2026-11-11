#ifndef PRDB_SDK_SQL_V20_H
#define PRDB_SDK_SQL_V20_H

#include "prdb_sdk_sql_v19.h"

#define PRDB_SDK_SQL_ABI_V20_VERSION UINT32_C(20)

/* V20 adds one qualified WHERE comparison to the alias-aware v19 INNER JOIN.
 * The predicate is `column comparison $n`, `column IS NULL`, or
 * `column IS NOT NULL`; LIMIT is applied after filtering. */
typedef struct prdb_sdk_select_v20 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_v19 base;
    uint32_t has_where;
    prdb_sdk_sql_qualified_column_v8 where_column;
    uint32_t comparison;
    uint32_t parameter_index;
    uint32_t where_keyword_offset;
    uint32_t where_clause_end;
    uint32_t from_keyword_offset;
} prdb_sdk_select_v20;

typedef struct prdb_sdk_select_binding_v20 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_binding_v19 base;
    uint32_t output_projection_count;
    uint32_t where_relation_side;
    uint32_t where_projection_index;
    prdb_sdk_sql_bound_column_v2 where_column;
} prdb_sdk_select_binding_v20;

int32_t prdb_sdk_sql_parse_select_v20(const uint8_t *sql, uint64_t sql_len,
                                      prdb_sdk_select_v20 *out,
                                      uint32_t out_capacity,
                                      uint32_t *error_offset_out);

int32_t prdb_sdk_sql_bind_select_v20(
    const prdb_sdk_select_v20 *statement,
    const uint8_t *left_relation_name, uint32_t left_relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *left_columns,
    uint32_t left_column_count,
    const uint8_t *right_relation_name, uint32_t right_relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *right_columns,
    uint32_t right_column_count, prdb_sdk_select_binding_v20 *out,
    uint32_t out_capacity);

#endif /* PRDB_SDK_SQL_V20_H */
