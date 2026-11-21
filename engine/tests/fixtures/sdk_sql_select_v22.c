#include "prdb_sdk_sql_v22.h"

#include <assert.h>
#include <string.h>

static int coefficient_equals(const uint8_t actual[16], int64_t expected) {
    uint8_t encoded[16];
    uint64_t bits = (uint64_t)expected;
    memset(encoded, expected < 0 ? 0xff : 0, sizeof(encoded));
    for (uint32_t index = 0; index < 8; ++index) {
        encoded[15 - index] = (uint8_t)bits;
        bits >>= 8;
    }
    return memcmp(actual, encoded, sizeof(encoded)) == 0;
}

int main(void) {
    static const uint8_t relation[] = "records";
    static const uint8_t key[] = "department", region[] = "region";
    static const uint8_t amount[] = "amount";
    static const uint8_t sql[] =
        "SELECT department, region, SUM(amount) FROM records "
        "GROUP BY department, region "
        "HAVING COUNT(*) >= 2 AND SUM(amount) >= 1.25 ORDER BY department";
    static const uint8_t parameter_sql[] =
        "SELECT department, region, SUM(amount) FROM records "
        "GROUP BY department, region HAVING COUNT(*) >= $1";
    prdb_sdk_select_v21 legacy = {0};
    prdb_sdk_sql_column_shape_v2 columns[3] = {
        {key, 10, 0, PRDB_SDK_VALUE_TYPE_TEXT, 0, 0},
        {region, 6, 1, PRDB_SDK_VALUE_TYPE_TEXT, 0, 0},
        {amount, 6, 2, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_select_v22 statement = {0};
    prdb_sdk_select_binding_v22 binding = {0};
    uint32_t error_offset = 0;

    assert(prdb_sdk_sql_parse_select_v22(
               sql, sizeof(sql) - 1, &statement, sizeof(statement),
               &error_offset) == PRDB_SDK_SQL_OK);
    assert(statement.abi_version == PRDB_SDK_SQL_ABI_V22_VERSION);
    assert(statement.base.base.having_node_count == 3);
    assert(statement.having_count_mask == UINT64_C(1));
    assert(prdb_sdk_sql_bind_select_v22(
               &statement, relation, sizeof(relation) - 1, columns, 3,
               &binding, sizeof(binding)) == PRDB_SDK_SQL_BIND_OK);
    assert(binding.base.base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding.having_count_mask == UINT64_C(1));
    assert(binding.base.having_nodes[0].scale == 0);
    assert(coefficient_equals(binding.base.having_nodes[0].coefficient, 2));
    assert(binding.base.having_nodes[1].scale == 2);
    assert(coefficient_equals(binding.base.having_nodes[1].coefficient, 125));
    assert(prdb_sdk_sql_parse_select_v22(
               parameter_sql, sizeof(parameter_sql) - 1, &statement,
               sizeof(statement), &error_offset) == PRDB_SDK_SQL_OK);
    assert(statement.having_count_mask == UINT64_C(1));
    assert(statement.base.having_nodes[0].base.operand_kind ==
           PRDB_SDK_SQL_V18_OPERAND_PARAMETER);
    assert(prdb_sdk_sql_bind_select_v22(
               &statement, relation, sizeof(relation) - 1, columns, 3,
               &binding, sizeof(binding)) == PRDB_SDK_SQL_BIND_OK);
    assert(binding.having_count_mask == UINT64_C(1));
    assert(prdb_sdk_sql_parse_select_v21(
               parameter_sql, sizeof(parameter_sql) - 1, &legacy,
               sizeof(legacy), &error_offset) == PRDB_SDK_SQL_SYNTAX_ERROR);
    return 0;
}
