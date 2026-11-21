#include "prdb_sdk_sql_v21.h"

#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_select_v21 statement;
    prdb_sdk_select_binding_v21 binding;
    uint32_t error_offset = 0;
    int32_t status;

    if (size > PRDB_SDK_SQL_MAX_QUERY_BYTES) return 0;
    memset(&statement, 0xa5, sizeof(statement));
    status = prdb_sdk_sql_parse_select_v21(
        data, size, &statement, sizeof(statement), &error_offset);
    if (status == PRDB_SDK_SQL_OK) {
        if (statement.struct_size != sizeof(statement) ||
            statement.abi_version != PRDB_SDK_SQL_ABI_V21_VERSION ||
            statement.base.having_node_count >
                PRDB_SDK_SQL_V18_MAX_HAVING_NODES ||
            (statement.base.having_node_count == 0 &&
             statement.base.having_root != 0) ||
            (statement.base.having_node_count != 0 &&
             statement.base.having_root >= statement.base.having_node_count))
            __builtin_trap();
        memset(&binding, 0x5a, sizeof(binding));
        status = prdb_sdk_sql_bind_select_v21(
            &statement,
            data + statement.base.base.base.base.base.base.relation.offset,
            statement.base.base.base.base.base.base.relation.length,
            columns, 2, &binding, sizeof(binding));
        if (status == PRDB_SDK_SQL_BIND_OK &&
            (binding.struct_size != sizeof(binding) ||
             binding.abi_version != PRDB_SDK_SQL_ABI_V21_VERSION ||
             binding.base.having_node_count !=
                 statement.base.having_node_count))
            __builtin_trap();
        if (status == PRDB_SDK_SQL_BIND_OK) {
            for (uint32_t index = 0;
                 index < binding.base.having_node_count; ++index) {
                const prdb_sdk_sql_having_node_v21 *node =
                    &binding.having_nodes[index];
                if (node->base.operand_kind !=
                    PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL)
                    continue;
                if (node->base.node_kind != PRDB_SDK_SQL_PREDICATE_COMPARE ||
                    node->base.parameter_index != 0 ||
                    node->base.literal != 0)
                    __builtin_trap();
                if (binding.base.having_type_code ==
                    PRDB_SDK_VALUE_TYPE_DECIMAL) {
                    if (binding.base.having_decimal_precision == 0 ||
                        binding.base.having_decimal_precision > 38 ||
                        binding.base.having_decimal_scale >
                            binding.base.having_decimal_precision ||
                        node->scale !=
                            binding.base.having_decimal_scale)
                        __builtin_trap();
                } else if (binding.base.having_type_code ==
                           PRDB_SDK_VALUE_TYPE_BIGINT) {
                    uint8_t extension =
                        (node->coefficient[8] & 0x80) != 0 ? 0xff : 0;
                    if (node->scale != 0)
                        __builtin_trap();
                    for (uint32_t byte = 0; byte < 8; ++byte)
                        if (node->coefficient[byte] != extension)
                            __builtin_trap();
                } else {
                    __builtin_trap();
                }
            }
        }
    }
    return 0;
}
