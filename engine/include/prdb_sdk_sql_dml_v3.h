#ifndef PRDB_SDK_SQL_DML_V3_H
#define PRDB_SDK_SQL_DML_V3_H

#include "prdb_sdk_sql_dml_v2.h"

#define PRDB_SDK_SQL_DML_ABI_V3_VERSION UINT32_C(3)
#define PRDB_SDK_SQL_MAX_INSERT_COLUMNS_V3 UINT32_C(4096)

/* V3 adds an optional explicit INSERT column list. A zero column_count keeps
 * the V2 positional schema-order form. Column spans borrow the SQL input. */
typedef struct prdb_sdk_insert_v3 {
    uint32_t struct_size;
    uint32_t abi_version;
    prdb_sdk_insert_v2 base;
    uint32_t column_count;
    prdb_sdk_sql_span_v1 columns[PRDB_SDK_SQL_MAX_INSERT_COLUMNS_V3];
} prdb_sdk_insert_v3;

int32_t prdb_sdk_sql_parse_insert_v3(const uint8_t *sql, uint64_t sql_len,
                                     prdb_sdk_insert_v3 *out,
                                     uint32_t out_capacity,
                                     uint32_t *error_offset_out);

#endif /* PRDB_SDK_SQL_DML_V3_H */
