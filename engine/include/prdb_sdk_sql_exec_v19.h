#ifndef PRDB_SDK_SQL_EXEC_V19_H
#define PRDB_SDK_SQL_EXEC_V19_H

#include "prdb_sdk_sql_exec_v16.h"
#include "prdb_sdk_sql_v19.h"

typedef int32_t (*prdb_sdk_sql_join_result_row_v19_fn)(
    void *context, const prdb_sdk_value_v1 *projected_values,
    uint32_t projection_count, const prdb_sdk_select_binding_v19 *binding);

/* Executes the v19 two-relation INNER JOIN over independently described and
 * resolved authorized relations. Distinct aliases can identify two scans of
 * the same relation. It preserves the v16 row, byte, scan-page, and
 * cancellation bounds. Projected value views and the binding are valid only
 * during the callback. Any nonzero callback status fails the statement;
 * earlier output is provisional and must
 * be discarded by the host transaction. All caller buffers, arrays, and row
 * storage must be disjoint. */
int32_t prdb_sdk_sql_execute_inner_join_v19(
    const prdb_host_api_v3 *host,
    const uint8_t *schema, uint32_t schema_len,
    const uint8_t *sql, uint64_t sql_len,
    uint16_t max_candidates, uint32_t max_batch_bytes,
    uint8_t *left_planner_response, uint64_t left_planner_response_capacity,
    uint8_t *right_planner_response, uint64_t right_planner_response_capacity,
    uint8_t *left_executor_response, uint64_t left_executor_response_capacity,
    uint8_t *right_executor_response, uint64_t right_executor_response_capacity,
    uint8_t *batch_response, uint64_t batch_response_capacity,
    prdb_sdk_sql_column_shape_v2 *left_planner_columns,
    uint32_t left_planner_column_capacity,
    prdb_sdk_sql_column_shape_v2 *right_planner_columns,
    uint32_t right_planner_column_capacity,
    prdb_sdk_sql_column_shape_v2 *left_executor_columns,
    uint32_t left_executor_column_capacity,
    prdb_sdk_sql_column_shape_v2 *right_executor_columns,
    uint32_t right_executor_column_capacity,
    prdb_sdk_value_v1 *cell_scratch, uint32_t cell_scratch_capacity,
    prdb_sdk_sql_join_materialized_row_v16 *left_rows,
    uint32_t left_row_capacity,
    prdb_sdk_sql_join_materialized_row_v16 *right_rows,
    uint32_t right_row_capacity,
    uint8_t *materialized_bytes, uint64_t materialized_bytes_capacity,
    prdb_sdk_sql_join_result_row_v19_fn on_row, void *result_context);

#endif /* PRDB_SDK_SQL_EXEC_V19_H */
