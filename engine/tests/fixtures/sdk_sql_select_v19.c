#include "prdb_sdk_sql_v19.h"

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
        "l.id = r.id AND l.region = r.region LIMIT 3";
    static const uint8_t duplicate_alias[] =
        "SELECT l.id FROM lefts AS l INNER JOIN rights AS L ON "
        "l.id = L.id AND l.region = L.region";
    static const uint8_t no_alias[] =
        "SELECT lefts.label, rights.value FROM lefts INNER JOIN rights ON "
        "lefts.id = rights.id AND lefts.region = rights.region";
    static const uint8_t missing_alias[] =
        "SELECT l.id FROM lefts AS INNER JOIN rights ON "
        "lefts.id = rights.id AND lefts.region = rights.region";
    static const uint8_t left_name[] = "lefts";
    static const uint8_t right_name[] = "rights";
    prdb_sdk_select_v19 statement = {0};
    prdb_sdk_select_v19 no_alias_statement = {0};
    prdb_sdk_select_v19 unchanged_statement = {0};
    prdb_sdk_select_v16 frozen_statement = {0};
    prdb_sdk_select_binding_v19 binding = {0};
    prdb_sdk_select_binding_v19 unchanged_binding = {0};
    prdb_sdk_sql_column_shape_v2 left_columns[] = {
        column("id", 0), column("region", 1), column("label", 2)};
    prdb_sdk_sql_column_shape_v2 right_columns[] = {
        column("id", 0), column("region", 1), column("value", 2)};
    uint32_t error_offset = 0;

    assert(prdb_sdk_sql_parse_select_v19(
               sql, sizeof(sql) - 1, &statement, sizeof(statement),
               &error_offset) == PRDB_SDK_SQL_OK);
    assert(statement.abi_version == PRDB_SDK_SQL_ABI_V19_VERSION &&
           statement.base.abi_version == PRDB_SDK_SQL_ABI_V16_VERSION &&
           statement.base.join_key_count == 2 &&
           statement.base.base.has_limit == 1 &&
           statement.base.base.limit == 3);
    assert(memcmp(sql + statement.left_alias.offset, "l", 1) == 0 &&
           memcmp(sql + statement.right_alias.offset, "r", 1) == 0);
    frozen_statement.struct_size = 57;
    assert(prdb_sdk_sql_parse_select_v16(
               sql, sizeof(sql) - 1, &frozen_statement,
               sizeof(frozen_statement), NULL) ==
           PRDB_SDK_SQL_SYNTAX_ERROR);
    assert(frozen_statement.struct_size == 57);
    assert(prdb_sdk_sql_bind_select_v19(
               &statement, left_name, sizeof(left_name) - 1, left_columns, 3,
               right_name, sizeof(right_name) - 1, right_columns, 3, &binding,
               sizeof(binding)) == PRDB_SDK_SQL_BIND_OK);
    assert(binding.abi_version == PRDB_SDK_SQL_ABI_V19_VERSION &&
           binding.base.join_key_count == 2 &&
           binding.base.base.projection_count == 2 &&
           binding.base.base.projections[0].relation_side ==
               PRDB_SDK_SQL_V8_LEFT &&
           binding.base.base.projections[0].column.ordinal == 2 &&
           binding.base.base.projections[1].relation_side ==
               PRDB_SDK_SQL_V8_RIGHT &&
           binding.base.base.projections[1].column.ordinal == 2 &&
           binding.base.join_keys[1].left.ordinal == 1 &&
           binding.base.join_keys[1].right.ordinal == 1);

    assert(prdb_sdk_sql_parse_select_v19(
               no_alias, sizeof(no_alias) - 1, &no_alias_statement,
               sizeof(no_alias_statement), NULL) == PRDB_SDK_SQL_OK);
    assert(no_alias_statement.left_alias.offset ==
               no_alias_statement.base.base.left_relation.offset &&
           no_alias_statement.left_alias.length ==
               no_alias_statement.base.base.left_relation.length &&
           no_alias_statement.right_alias.offset ==
               no_alias_statement.base.base.right_relation.offset &&
           no_alias_statement.right_alias.length ==
               no_alias_statement.base.base.right_relation.length);
    assert(prdb_sdk_sql_bind_select_v19(
               &no_alias_statement, left_name, sizeof(left_name) - 1,
               left_columns, 3, right_name, sizeof(right_name) - 1,
               right_columns, 3, &binding, sizeof(binding)) ==
           PRDB_SDK_SQL_BIND_OK);

    unchanged_statement.struct_size = 73;
    assert(prdb_sdk_sql_parse_select_v19(
               duplicate_alias, sizeof(duplicate_alias) - 1,
               &unchanged_statement, sizeof(unchanged_statement), NULL) ==
           PRDB_SDK_SQL_OK);
    unchanged_binding.struct_size = 81;
    assert(prdb_sdk_sql_bind_select_v19(
               &unchanged_statement, left_name, sizeof(left_name) - 1,
               left_columns, 3, right_name, sizeof(right_name) - 1,
               right_columns, 3, &unchanged_binding,
               sizeof(unchanged_binding)) ==
           PRDB_SDK_SQL_BIND_INVALID_STATEMENT);
    assert(unchanged_binding.struct_size == 81);

    unchanged_statement.struct_size = 91;
    assert(prdb_sdk_sql_parse_select_v19(
               missing_alias, sizeof(missing_alias) - 1,
               &unchanged_statement, sizeof(unchanged_statement),
               &error_offset) == PRDB_SDK_SQL_SYNTAX_ERROR);
    assert(unchanged_statement.struct_size == 91 && error_offset != 0);

    statement.left_alias.offset = UINT32_MAX;
    assert(prdb_sdk_sql_bind_select_v19(
               &statement, left_name, sizeof(left_name) - 1, left_columns, 3,
               right_name, sizeof(right_name) - 1, right_columns, 3,
               &unchanged_binding, sizeof(unchanged_binding)) ==
           PRDB_SDK_SQL_BIND_INVALID_STATEMENT);
    assert(unchanged_binding.struct_size == 81);
    return 0;
}
