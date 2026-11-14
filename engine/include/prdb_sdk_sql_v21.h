#ifndef PRDB_SDK_SQL_V21_H
#define PRDB_SDK_SQL_V21_H

#include "prdb_sdk_sql_v18.h"

#define PRDB_SDK_SQL_ABI_V21_VERSION UINT32_C(21)
#define PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL UINT32_C(3)
#define PRDB_SDK_SQL_BIND_V21_LITERAL_TYPE_MISMATCH \
    PRDB_SDK_SQL_BIND_V18_LITERAL_TYPE_MISMATCH

/* Numeric literal coefficient is signed big-endian two's complement. */
typedef struct prdb_sdk_sql_having_node_v21 {
    prdb_sdk_sql_having_node_v18 base;
    uint8_t coefficient[16];
    uint8_t scale;
    uint8_t reserved[7];
} prdb_sdk_sql_having_node_v21;

typedef struct prdb_sdk_select_v21 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_v18 base;
    prdb_sdk_sql_having_node_v21
        having_nodes[PRDB_SDK_SQL_V18_MAX_HAVING_NODES];
} prdb_sdk_select_v21;

typedef struct prdb_sdk_select_binding_v21 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_select_binding_v18 base;
    prdb_sdk_sql_having_node_v21
        having_nodes[PRDB_SDK_SQL_V18_MAX_HAVING_NODES];
} prdb_sdk_select_binding_v21;

int32_t prdb_sdk_sql_parse_select_v21(const uint8_t *sql, uint64_t sql_len,
                                      prdb_sdk_select_v21 *out,
                                      uint32_t out_capacity,
                                      uint32_t *error_offset_out);

int32_t prdb_sdk_sql_bind_select_v21(
    const prdb_sdk_select_v21 *statement, const uint8_t *relation_name,
    uint32_t relation_name_len,
    const prdb_sdk_sql_column_shape_v2 *columns, uint32_t column_count,
    prdb_sdk_select_binding_v21 *out, uint32_t out_capacity);

#endif /* PRDB_SDK_SQL_V21_H */
