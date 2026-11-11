#include "prdb_sdk_sql_v20.h"

#include <assert.h>
#include <string.h>

static prdb_sdk_sql_column_shape_v2 column(const char *name,
                                          uint32_t ordinal) {
    return (prdb_sdk_sql_column_shape_v2){
        (const uint8_t *)name, (uint32_t)strlen(name), ordinal,
        PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0};
}

int main(void) {
    static const uint8_t sql[] =
        "SELECT l.label, r.value FROM lefts AS l INNER JOIN rights AS r ON "
        "l.id = r.id AND l.region = r.region WHERE r.value >= $1 LIMIT 2";
    static const uint8_t invalid_parameter[] =
        "SELECT l.label FROM lefts AS l INNER JOIN rights AS r ON "
        "l.id = r.id AND l.region = r.region WHERE r.value = $0";
    static const uint8_t left_name[] = "lefts";
    static const uint8_t right_name[] = "rights";
    prdb_sdk_sql_column_shape_v2 left_columns[] = {
        column("id", 0), column("region", 1), column("label", 2)};
    prdb_sdk_sql_column_shape_v2 right_columns[] = {
        column("id", 0), column("region", 1), column("value", 2)};
    prdb_sdk_select_v20 statement = {0};
    prdb_sdk_select_binding_v20 binding = {0};
    prdb_sdk_select_v19 frozen = {0};
    uint32_t error_offset = 0;

    assert(prdb_sdk_sql_parse_select_v20(
               sql, sizeof(sql) - 1, &statement, sizeof(statement),
               &error_offset) == PRDB_SDK_SQL_OK);
    assert(statement.abi_version == PRDB_SDK_SQL_ABI_V20_VERSION &&
           statement.base.abi_version == PRDB_SDK_SQL_ABI_V19_VERSION &&
           statement.has_where == 1 &&
           statement.comparison == PRDB_SDK_SQL_COMPARE_GREATER_EQUAL &&
           statement.parameter_index == 1 &&
           statement.base.base.base.has_limit == 1 &&
           statement.base.base.base.limit == 2);
    assert(prdb_sdk_sql_bind_select_v20(
               &statement, left_name, sizeof(left_name) - 1, left_columns, 3,
               right_name, sizeof(right_name) - 1, right_columns, 3, &binding,
               sizeof(binding)) == PRDB_SDK_SQL_BIND_OK);
    assert(binding.abi_version == PRDB_SDK_SQL_ABI_V20_VERSION &&
           binding.output_projection_count == 2 &&
           binding.where_relation_side == PRDB_SDK_SQL_V8_RIGHT &&
           binding.where_column.ordinal == 2 &&
           binding.where_projection_index == 2 &&
           binding.base.base.base.projection_count == 3);

    assert(prdb_sdk_sql_parse_select_v19(
               sql, sizeof(sql) - 1, &frozen, sizeof(frozen), NULL) ==
           PRDB_SDK_SQL_SYNTAX_ERROR);
    statement.struct_size = 91;
    assert(prdb_sdk_sql_parse_select_v20(
               invalid_parameter, sizeof(invalid_parameter) - 1, &statement,
               sizeof(statement), &error_offset) == PRDB_SDK_SQL_SYNTAX_ERROR);
    assert(statement.struct_size == 91 && error_offset != 0);
    return 0;
}
