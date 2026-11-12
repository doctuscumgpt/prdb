#include "prdb_sdk_sql_exec_v2.h"
#include "prdb_sdk_sql_exec_v3.h"
#include "prdb_sdk_sql_exec_v5.h"
#include "prdb_sdk_sql_exec_v6.h"
#include "prdb_sdk_sql_exec_v7.h"
#include "prdb_sdk_sql_exec_v8.h"
#include "prdb_sdk_sql_exec_v10.h"
#include "prdb_sdk_sql_exec_v11.h"
#include "prdb_sdk_sql_exec_v12.h"
#include "prdb_sdk_sql_exec_v13.h"
#include "prdb_sdk_sql_exec_v15.h"
#include "prdb_sdk_sql_exec_v17.h"
#include "prdb_sdk_sql_exec_v18.h"
#include "prdb_sdk_sql_exec_v16.h"
#include "prdb_sdk_sql_exec_v19.h"
#include "prdb_sdk_sql_exec_v20.h"
#include "prdb_sdk_sql_exec_v4.h"
#include "prdb_sdk_sql_result_v1.h"
#include "prdb_sdk_executor_v1.h"
#include "prdb_sdk_optimizer_plan_v1.h"
#include "prdb_sdk_optimizer_v1.h"
#include "prdb_sdk_optimizer_v2.h"

#include <string.h>
#include <time.h>

const prdb_query_descriptor_v3_record prdb_query_descriptor_v3 = {
    sizeof(prdb_query_descriptor_v3_record), PRDB_QUERY_ABI_V3_VERSION,
    sizeof(prdb_host_api_v3), sizeof(prdb_invocation_v3)};
const prdb_query_descriptor_v4_record prdb_query_descriptor_v4 = {
    sizeof(prdb_query_descriptor_v4_record), PRDB_QUERY_ABI_V4_VERSION,
    sizeof(prdb_host_api_v4), sizeof(prdb_invocation_v4)};
const prdb_query_descriptor_v5_record prdb_query_descriptor_v5 = {
    sizeof(prdb_query_descriptor_v5_record), PRDB_QUERY_ABI_V5_VERSION,
    sizeof(prdb_host_api_v5), sizeof(prdb_invocation_v5)};

static const prdb_host_api_v5 *cancel_update_original_host;
static uint32_t cancel_update_ready_sent;
static uint32_t fail_next_stage_update;

static int32_t cancel_update_executor_request(
    void *context, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len) {
    if (cancel_update_original_host == NULL ||
        context != cancel_update_original_host->v4.v3.context ||
        cancel_update_original_host->v4.v3.executor_request == NULL ||
        cancel_update_original_host->v4.v3.write_result == NULL ||
        response == NULL || response_len == NULL) {
        return PRDB_QUERY_CALLBACK_INVALID_ARGUMENT;
    }
    if (operation == PRDB_IPC_OP_STAGE_UPDATE &&
        fail_next_stage_update != 0) {
        static const uint8_t injected[] = "UPDATE_STAGE_FAILURE_INJECTED";
        fail_next_stage_update = 0;
        (void)cancel_update_original_host->v4.v3.write_result(
            context, injected, sizeof(injected) - 1);
        return PRDB_QUERY_CALLBACK_IO_ERROR;
    }
    if (operation != PRDB_IPC_OP_EXECUTOR_CHECK_CANCELLATION ||
        cancel_update_ready_sent != 0) {
        return cancel_update_original_host->v4.v3.executor_request(
            context, operation, request, request_len, response,
            response_capacity, response_len);
    }

    static const uint8_t ready[] = "CANCEL_READY";
    static const uint8_t observed[] = "CANCEL_OBSERVED";
    if (cancel_update_original_host->v4.v3.write_result(
            context, ready, sizeof(ready) - 1) != 0) {
        return PRDB_QUERY_CALLBACK_IO_ERROR;
    }
    cancel_update_ready_sent = 1;
    const struct timespec poll_interval = {0, 1000000};
    for (uint32_t poll = 0; poll < 200; ++poll) {
        if (nanosleep(&poll_interval, NULL) != 0) {
            return PRDB_QUERY_CALLBACK_IO_ERROR;
        }
        int32_t status = cancel_update_original_host->v4.v3.executor_request(
            context, operation, request, request_len, response,
            response_capacity, response_len);
        if (status != PRDB_QUERY_CALLBACK_OK) return status;
        if (*response_len == 3 && response_capacity >= 3 && response[2] == 1) {
            return cancel_update_original_host->v4.v3.write_result(
                       context, observed, sizeof(observed) - 1) == 0
                       ? PRDB_QUERY_CALLBACK_OK
                       : PRDB_QUERY_CALLBACK_IO_ERROR;
        }
    }
    return PRDB_QUERY_CALLBACK_LIMIT_EXCEEDED;
}

typedef struct optimizer_result_context {
    uint32_t row_count;
    uint32_t matched_expected_value;
    uint32_t expected_value;
} optimizer_result_context;

typedef struct ordered_optimizer_result_context {
    const prdb_host_api_v3 *host;
    const prdb_sdk_sql_column_shape_v2 *columns;
    const prdb_sdk_executor_relation_v1 *relation;
    uint8_t *record_buffer;
    uint64_t record_capacity;
    uint32_t row_count;
    uint32_t expected_values[2];
} ordered_optimizer_result_context;

static int32_t verify_optimizer_row(
    void *opaque, const prdb_sdk_optimizer_plan_column_v1 *projections,
    const prdb_sdk_value_v1 *values, uint32_t projection_count) {
    optimizer_result_context *context = (optimizer_result_context *)opaque;
    uint8_t expected_value[4] = {0, 0, 0, 42};
    if (context == NULL || projections == NULL || values == NULL ||
        projection_count != 1 ||
        projections[0].type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
        values[0].state != PRDB_SDK_VALUE_STATE_VALUE ||
        values[0].type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
        values[0].data == NULL || values[0].data_len != sizeof(expected_value)) {
        return 1;
    }
    if (context->expected_value != 0) {
        expected_value[0] = (uint8_t)(context->expected_value >> 24);
        expected_value[1] = (uint8_t)(context->expected_value >> 16);
        expected_value[2] = (uint8_t)(context->expected_value >> 8);
        expected_value[3] = (uint8_t)context->expected_value;
    }
    context->matched_expected_value =
        memcmp(values[0].data, expected_value, sizeof(expected_value)) == 0;
    ++context->row_count;
    return context->matched_expected_value ? 0 : 1;
}

static int32_t emit_ordered_optimizer_row(
    void *opaque, const prdb_sdk_optimizer_plan_column_v1 *projections,
    const prdb_sdk_value_v1 *values, uint32_t projection_count) {
    ordered_optimizer_result_context *context = opaque;
    uint32_t ordinals[PRDB_SDK_OPTIMIZER_PLAN_MAX_PROJECTIONS];
    uint8_t expected_bytes[4];
    uint64_t record_len = 0;
    uint32_t expected_value;
    if (context != NULL && context->row_count < 2) {
        expected_value = context->expected_values[context->row_count];
        expected_bytes[0] = (uint8_t)(expected_value >> 24);
        expected_bytes[1] = (uint8_t)(expected_value >> 16);
        expected_bytes[2] = (uint8_t)(expected_value >> 8);
        expected_bytes[3] = (uint8_t)expected_value;
    } else {
        memset(expected_bytes, 0, sizeof(expected_bytes));
    }
    if (context == NULL || context->host == NULL || context->columns == NULL ||
        context->relation == NULL || projections == NULL || values == NULL ||
        projection_count != 1 || context->row_count >= 2 ||
        values[0].state != PRDB_SDK_VALUE_STATE_VALUE ||
        values[0].type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
        values[0].data == NULL || values[0].data_len != 4 ||
        memcmp(values[0].data, expected_bytes, sizeof(expected_bytes)) != 0) {
        return -1;
    }
    ordinals[0] = projections[0].ordinal;
    if (prdb_sdk_sql_write_projected_result_v1(
            context->host, context->columns, context->relation->column_count,
            ordinals, values, projection_count, context->record_buffer,
            context->record_capacity, &record_len) != PRDB_SDK_SQL_RESULT_OK) {
        return -1;
    }
    ++context->row_count;
    return 0;
}

typedef struct result_context {
    const prdb_host_api_v3 *host;
    uint32_t expected_state;
    const uint8_t *expected_value;
    uint64_t expected_value_len;
    const uint8_t *marker;
    uint64_t marker_len;
    uint32_t row_count;
    uint32_t expected_ordinal;
    uint32_t failure_code;
} result_context;

typedef struct grouped_result_context {
    const prdb_host_api_v3 *host;
    const prdb_sdk_sql_column_shape_v2 *group_column;
    const prdb_sdk_sql_column_shape_v2 *aggregate_column;
    uint8_t *output;
    uint64_t output_capacity;
    uint64_t output_len;
    uint32_t row_count;
} grouped_result_context;

typedef struct joined_result_context {
    const prdb_host_api_v3 *host;
    const prdb_sdk_sql_column_shape_v2 *left_columns;
    const prdb_sdk_sql_column_shape_v2 *right_columns;
    uint8_t *output;
    uint64_t output_capacity;
    uint64_t output_len;
    uint32_t row_count;
} joined_result_context;

typedef struct composite_grouped_result_context {
    const prdb_host_api_v3 *host;
    const prdb_sdk_sql_column_shape_v2 *columns;
    uint32_t column_count;
    uint8_t *output;
    uint64_t output_capacity;
    uint64_t output_len;
    uint32_t row_count;
} composite_grouped_result_context;

static int32_t write_grouped_composite_count(
    void *opaque, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v13 *binding) {
    composite_grouped_result_context *context =
        (composite_grouped_result_context *)opaque;
    if (context == NULL || binding == NULL || aggregate_value != NULL ||
        binding->base.aggregate_kind !=
            PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR ||
        group_key_count != 2 || binding->group_key_count != 2 ||
        matching_row_count == 0) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_group_composite_result_v1(
        context->host, binding, context->columns, context->column_count,
        group_keys, group_key_count, NULL, matching_row_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_grouped_composite_count_v15(
    void *opaque, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v15 *binding) {
    if (binding == NULL || binding->order_count != 3 ||
        binding->order_items[0].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_GROUP_KEY ||
        binding->order_items[0].item.group_key_index != 1 ||
        binding->order_items[1].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_AGGREGATE ||
        binding->order_items[2].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_GROUP_KEY ||
        binding->order_items[2].item.group_key_index != 0) {
        return 1;
    }
    return write_grouped_composite_count(
        opaque, group_keys, group_key_count, aggregate_value,
        matching_row_count, &binding->base.base);
}

static int32_t write_grouped_composite_count_v17(
    void *opaque, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v17 *binding) {
    if (binding == NULL || binding->has_having_literal != 1 ||
        binding->having_literal != UINT64_C(1)) {
        return 1;
    }
    return write_grouped_composite_count(
        opaque, group_keys, group_key_count, aggregate_value,
        matching_row_count, &binding->base.base.base);
}

static int32_t write_grouped_composite_count_v18(
    void *opaque, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v18 *binding) {
    if (binding == NULL || binding->having_node_count != 6 ||
        binding->having_root != 5 ||
        binding->having_type_code != PRDB_SDK_VALUE_TYPE_BIGINT ||
        binding->base.base.has_having != 0) {
        return 1;
    }
    return write_grouped_composite_count(
        opaque, group_keys, group_key_count, aggregate_value,
        matching_row_count, &binding->base.base.base);
}

static int32_t write_joined_row(void *opaque,
                                const prdb_sdk_value_v1 *projected_values,
                                uint32_t projection_count,
                                const prdb_sdk_select_binding_v8 *binding) {
    joined_result_context *context = (joined_result_context *)opaque;
    if (context == NULL || binding == NULL || projection_count != 2 ||
        binding->projection_count != 2) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_join_result_v1(
        context->host, binding, context->left_columns, 1,
        context->right_columns, 2, projected_values, projection_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_composite_joined_row(
    void *opaque, const prdb_sdk_value_v1 *projected_values,
    uint32_t projection_count, const prdb_sdk_select_binding_v16 *binding) {
    joined_result_context *context = (joined_result_context *)opaque;
    if (context == NULL || binding == NULL ||
        binding->join_key_count != 2 || projection_count != 4 ||
        binding->base.projection_count != 4) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_join_result_v1(
        context->host, &binding->base, context->left_columns, 2,
        context->right_columns, 2, projected_values, projection_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_composite_joined_row_v19(
    void *opaque, const prdb_sdk_value_v1 *projected_values,
    uint32_t projection_count,
    const prdb_sdk_select_binding_v19 *binding) {
    if (binding == NULL) return 1;
    return write_composite_joined_row(opaque, projected_values,
                                      projection_count, &binding->base);
}

static int32_t write_self_joined_row_v19(
    void *opaque, const prdb_sdk_value_v1 *projected_values,
    uint32_t projection_count,
    const prdb_sdk_select_binding_v19 *binding) {
    joined_result_context *context = (joined_result_context *)opaque;
    if (context == NULL || binding == NULL || projection_count != 2 ||
        binding->base.base.projection_count != 2 ||
        binding->base.join_key_count != 2) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_join_result_v1(
        context->host, &binding->base.base, context->left_columns, 2,
        context->right_columns, 2, projected_values, projection_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_filtered_joined_row_v20(
    void *opaque, const prdb_sdk_value_v1 *projected_values,
    uint32_t projection_count,
    const prdb_sdk_select_binding_v20 *binding) {
    joined_result_context *context = (joined_result_context *)opaque;
    if (context == NULL || binding == NULL || projection_count != 2 ||
        binding->output_projection_count != 2 ||
        binding->base.base.base.projection_count != 2 ||
        binding->base.base.join_key_count != 2) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_join_result_v1(
        context->host, &binding->base.base.base, context->left_columns, 2,
        context->right_columns, 2, projected_values, projection_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_grouped_min(void *opaque,
                                 const prdb_sdk_value_v1 *group_key,
                                 const prdb_sdk_value_v1 *aggregate_value,
                                 uint64_t matching_row_count,
                                 const prdb_sdk_select_binding_v5 *binding) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    if (context == NULL || binding == NULL ||
        binding->aggregate_kind != PRDB_SDK_SQL_V5_AGGREGATE_MIN ||
        matching_row_count != 1) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_group_minmax_result_v1(
        context->host, context->group_column, group_key,
        context->aggregate_column, aggregate_value, binding->aggregate_kind,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_optimizer_group_count(
    void *opaque, const prdb_sdk_value_v1 *group_key, uint64_t count,
    const prdb_sdk_sql_bound_column_v2 *group_by_column) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    if (context == NULL || context->host == NULL || group_key == NULL ||
        group_by_column == NULL || count != 1 ||
        group_by_column->ordinal != 0 ||
        group_by_column->type_code != PRDB_SDK_VALUE_TYPE_INTEGER)
        return 1;
    if (prdb_sdk_sql_write_group_count_result_v1(
            context->host, context->group_column, group_key, count,
            context->output, context->output_capacity,
            &context->output_len) != PRDB_SDK_SQL_RESULT_OK)
        return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_grouped_sum(void *opaque,
                                 const prdb_sdk_value_v1 *group_key,
                                 const prdb_sdk_value_v1 *aggregate_value,
                                 uint64_t matching_row_count,
                                 const prdb_sdk_select_binding_v6 *binding) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    if (context == NULL || binding == NULL ||
        binding->aggregate_kind != PRDB_SDK_SQL_V6_AGGREGATE_SUM ||
        matching_row_count != 1) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_group_sum_result_v1(
        context->host, context->group_column, group_key,
        context->aggregate_column, aggregate_value, context->output,
        context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_grouped_avg(void *opaque,
                                 const prdb_sdk_value_v1 *group_key,
                                 const prdb_sdk_value_v1 *aggregate_value,
                                 uint64_t matching_row_count,
                                 const prdb_sdk_select_binding_v7 *binding) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    if (context == NULL || binding == NULL ||
        binding->aggregate_kind != PRDB_SDK_SQL_V7_AGGREGATE_AVG ||
        matching_row_count != 1) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_group_avg_result_v1(
        context->host, context->group_column, group_key,
        context->aggregate_column, aggregate_value, context->output,
        context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_ordered_grouped_avg(
    void *opaque, const prdb_sdk_value_v1 *group_key,
    const prdb_sdk_value_v1 *aggregate_value, uint64_t matching_row_count,
    const prdb_sdk_select_binding_v12 *binding) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    static const int32_t expected_keys[] = {9, 7};
    static const uint64_t expected_counts[] = {1, 2};
    int32_t key;
    if (context == NULL || binding == NULL || group_key == NULL ||
        context->row_count >= 2 || binding->order_count != 2 ||
        binding->base.base.base.base.aggregate_kind !=
            PRDB_SDK_SQL_V7_AGGREGATE_AVG ||
        binding->order_items[0].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_AGGREGATE ||
        binding->order_items[0].item.direction !=
            PRDB_SDK_SQL_V10_ORDER_DESCENDING ||
        binding->order_items[0].item.null_order !=
            PRDB_SDK_SQL_V11_NULLS_LAST ||
        binding->order_items[1].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_GROUP_KEY ||
        binding->order_items[1].item.direction !=
            PRDB_SDK_SQL_V10_ORDER_ASCENDING ||
        binding->order_items[1].item.null_order !=
            PRDB_SDK_SQL_V11_NULLS_FIRST ||
        prdb_sdk_value_get_integer_v1(group_key, &key) !=
            PRDB_SDK_VALUE_ACCESS_OK ||
        key != expected_keys[context->row_count] ||
        matching_row_count != expected_counts[context->row_count])
        return 1;
    int32_t status = prdb_sdk_sql_write_group_avg_result_v1(
        context->host, context->group_column, group_key,
        context->aggregate_column, aggregate_value, context->output,
        context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t write_grouped_ordered_count(
    void *opaque, const prdb_sdk_value_v1 *group_key,
    const prdb_sdk_value_v1 *aggregate_value, uint64_t matching_row_count,
    const prdb_sdk_select_binding_v12 *binding) {
    grouped_result_context *context = (grouped_result_context *)opaque;
    if (context == NULL || binding == NULL || aggregate_value != NULL ||
        binding->base.base.base.base.aggregate_kind !=
            PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR ||
        binding->base.base.has_order_by != 1 ||
        binding->order_count != 2 ||
        binding->order_items[0].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_AGGREGATE ||
        binding->order_items[0].item.direction !=
            PRDB_SDK_SQL_V10_ORDER_DESCENDING ||
        binding->order_items[0].item.null_order !=
            PRDB_SDK_SQL_V11_NULLS_LAST ||
        binding->order_items[1].item.kind !=
            PRDB_SDK_SQL_V10_ORDER_GROUP_KEY ||
        binding->order_items[1].item.direction !=
            PRDB_SDK_SQL_V10_ORDER_ASCENDING ||
        binding->order_items[1].item.null_order !=
            PRDB_SDK_SQL_V11_NULLS_LAST ||
        matching_row_count != 1) {
        return 1;
    }
    int32_t status = prdb_sdk_sql_write_group_count_result_v1(
        context->host, context->group_column, group_key, matching_row_count,
        context->output, context->output_capacity, &context->output_len);
    if (status != PRDB_SDK_SQL_RESULT_OK) return 1;
    ++context->row_count;
    return 0;
}

static int32_t verify_selected_value(void *opaque, const prdb_sdk_row_v1 *row,
                                     const prdb_sdk_select_binding_v2 *binding) {
    result_context *context = (result_context *)opaque;
    prdb_sdk_cell_iterator_v1 cells = {0};
    prdb_sdk_value_v1 value = {0};

    (void)binding;
    if (context == NULL || row == NULL || row->cell_count != 1 ||
        prdb_sdk_row_cells_begin_v1(row, &cells, sizeof(cells)) !=
            PRDB_SDK_BATCH_OK ||
        prdb_sdk_cell_next_v1(&cells, &value, sizeof(value)) !=
            PRDB_SDK_BATCH_OK ||
        value.state != context->expected_state ||
        cells.cells_remaining != 0) {
        return 1;
    }
    if (value.state == PRDB_SDK_VALUE_STATE_VALUE &&
        (value.type_code != PRDB_SDK_VALUE_TYPE_INTEGER ||
         value.data_len != context->expected_value_len ||
         memcmp(value.data, context->expected_value,
                (size_t)context->expected_value_len) != 0)) {
        return 1;
    }
    ++context->row_count;
    if (context->marker != NULL) {
        return context->host->write_result(context->host->context,
                                           context->marker,
                                           context->marker_len);
    }
    return 0;
}

static int32_t verify_information_schema_name(
    void *opaque, const prdb_sdk_row_v1 *row,
    const prdb_sdk_select_binding_v2 *binding) {
    result_context *context = (result_context *)opaque;
    prdb_sdk_cell_iterator_v1 cells = {0};
    prdb_sdk_value_v1 value = {0};

    if (context == NULL || row == NULL || binding == NULL ||
        binding->projection_count != 1 ||
        binding->projection_ordinals[0] != context->expected_ordinal ||
        row->cell_count <= context->expected_ordinal ||
        prdb_sdk_row_cells_begin_v1(row, &cells, sizeof(cells)) !=
            PRDB_SDK_BATCH_OK) {
        if (context != NULL) context->failure_code = 1;
        return 1;
    }
    for (uint32_t index = 0; index <= context->expected_ordinal; ++index) {
        if (prdb_sdk_cell_next_v1(&cells, &value, sizeof(value)) !=
            PRDB_SDK_BATCH_OK) {
            context->failure_code = 2;
            return 1;
        }
    }
    if (
        value.state != PRDB_SDK_VALUE_STATE_VALUE ||
        value.type_code != PRDB_SDK_VALUE_TYPE_TEXT ||
        (context->expected_value != NULL &&
         (value.data_len != context->expected_value_len ||
          memcmp(value.data, context->expected_value,
                 (size_t)context->expected_value_len) != 0))) {
        context->failure_code = 3;
        return 1;
    }
    ++context->row_count;
    if (context->marker != NULL) {
        return context->host->write_result(context->host->context,
                                           context->marker,
                                           context->marker_len);
    }
    return context->host->write_result(context->host->context, value.data,
                                       value.data_len);
}

static int32_t report_failure(const prdb_host_api_v3 *host,
                              const uint8_t *message, uint64_t length) {
    (void)host->write_result(host->context, message, length);
    return 1;
}

static int32_t run_write_read_fixture(
    const prdb_host_api_v3 *host, const prdb_invocation_v3 *invocation,
    const prdb_host_api_v4 *optimizer_host,
    const prdb_host_api_v5 *optimizer_host_v5) {
    static const uint8_t insert_sql[] = "INSERT INTO records VALUES (NULL)";
    static const uint8_t select_null_sql[] =
        "SELECT id FROM records WHERE id IS NULL";
    static const uint8_t select_schemata_sql[] =
        "SELECT schema_name FROM schemata WHERE schema_name = $1 LIMIT 1";
    static const uint8_t select_tables_sql[] =
        "SELECT table_name FROM tables WHERE table_schema = $1 LIMIT 1";
    static const uint8_t select_columns_sql[] =
        "SELECT column_name FROM columns WHERE table_schema = $1 LIMIT 1";
    static const uint8_t select_records_columns_sql[] =
        "SELECT column_name FROM columns WHERE table_schema = $1 AND "
        "table_name = $2 LIMIT 1";
    static const uint8_t select_private_table_sql[] =
        "SELECT table_name FROM tables WHERE table_schema = $1 AND table_name = $2 LIMIT 1";
    static const uint8_t select_private_columns_sql[] =
        "SELECT column_name FROM columns WHERE table_schema = $1 AND table_name = $2 LIMIT 1";
    static const uint8_t update_sql[] =
        "UPDATE records SET id = $1 WHERE id IS NULL";
    static const uint8_t select_value_sql[] =
        "SELECT id FROM records WHERE id = $1";
    static const uint8_t optimizer_select_sql[] =
        "SELECT id FROM records WHERE id = $1 LIMIT 1";
    static const uint8_t optimizer_range_select_sql[] =
        "SELECT left_key FROM grouped_records "
        "WHERE left_key >= $1 AND left_key < $2 LIMIT 1";
    static const uint8_t optimizer_ordered_select_sql[] =
        "SELECT right_key FROM grouped_records "
        "ORDER BY left_key DESC LIMIT 2";
    static const uint8_t select_grouped_min_sql[] =
        "SELECT id, MIN(id) FROM records GROUP BY id";
    static const uint8_t select_grouped_sum_sql[] =
        "SELECT id, SUM(id) FROM records GROUP BY id";
    static const uint8_t select_grouped_avg_sql[] =
        "SELECT id, AVG(id) FROM records GROUP BY id";
    static const uint8_t select_ordered_grouped_avg_sql[] =
        "SELECT right_key, AVG(left_key) FROM grouped_records "
        "GROUP BY right_key ORDER BY AVG(left_key) DESC NULLS LAST, "
        "right_key ASC NULLS FIRST LIMIT 2";
    static const uint8_t select_optimizer_group_count_sql[] =
        "SELECT id, COUNT(*) FROM records GROUP BY id";
    static const uint8_t select_grouped_order_sql[] =
        "SELECT id, COUNT(*) FROM records GROUP BY id "
        "ORDER BY COUNT(*) DESC NULLS LAST, id ASC NULLS LAST LIMIT 1";
    static const uint8_t select_composite_group_sql[] =
        "SELECT left_key, right_key, COUNT(*) FROM grouped_records "
        "GROUP BY left_key, right_key LIMIT 2";
    static const uint8_t select_composite_order_sql[] =
        "SELECT left_key, right_key, COUNT(*) FROM grouped_records "
        "GROUP BY left_key, right_key "
        "ORDER BY right_key DESC NULLS LAST, COUNT(*) DESC, "
        "left_key ASC NULLS LAST LIMIT 2";
    static const uint8_t select_composite_literal_sql[] =
        "SELECT left_key, right_key, COUNT(*) FROM grouped_records "
        "GROUP BY left_key, right_key HAVING COUNT(*) >= 1 "
        "ORDER BY right_key DESC NULLS LAST, COUNT(*) DESC, "
        "left_key ASC NULLS LAST LIMIT 2";
    static const uint8_t select_composite_boolean_sql[] =
        "SELECT left_key, right_key, COUNT(*) FROM grouped_records "
        "GROUP BY left_key, right_key HAVING "
        "(COUNT(*) >= 2 OR COUNT(*) = 3) AND NOT COUNT(*) < 2 "
        "ORDER BY right_key DESC NULLS LAST, COUNT(*) DESC, "
        "left_key ASC NULLS LAST LIMIT 2";
    static const uint8_t select_join_sql[] =
        "SELECT records.id, linked_records.id FROM records INNER JOIN "
        "linked_records ON records.id = linked_records.id LIMIT 1";
    static const uint8_t select_composite_join_sql[] =
        "SELECT g.left_key, g.right_key, l.id, l.match_key "
        "FROM grouped_records AS g INNER JOIN linked_records AS l ON "
        "g.left_key = l.id AND g.right_key = l.match_key LIMIT 2";
    static const uint8_t select_self_join_sql[] =
        "SELECT a.id, b.id FROM linked_records AS a INNER JOIN "
        "linked_records AS b ON a.id = b.id AND "
        "a.match_key = b.match_key";
    static const uint8_t select_filtered_composite_join_sql[] =
        "SELECT g.left_key, l.id FROM grouped_records AS g INNER JOIN "
        "linked_records AS l ON g.right_key = l.match_key AND "
        "g.right_key = l.match_key WHERE l.id >= $1 LIMIT 1";
    static const uint8_t filtered_join_parameter_bytes[] = {0, 0, 0, 42};
    const prdb_sdk_value_v1 filtered_join_parameter = {
        sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
        filtered_join_parameter_bytes, sizeof(filtered_join_parameter_bytes)};
    static const uint8_t delete_sql[] = "DELETE FROM records WHERE id = $1";
    static const uint8_t multi_insert_sql[] =
        "INSERT INTO records VALUES ($1), ($2)";
    static const uint8_t schema[] = "sql_roundtrip";
    static const uint8_t empty_schema[] = "empty_roundtrip";
    static const uint8_t information_schema[] = "information_schema";
    static const uint8_t expected_user_column[] = "id";
    static const uint8_t records_table_name[] = "records";
    static const uint8_t private_table_name[] = "private_records";
    static const uint8_t missing_table_name[] = "missing_records";
    static const uint8_t hidden_schema_name[] = "hidden_roundtrip";
    static const uint8_t missing_schema_name[] = "missing_roundtrip";
    static const uint8_t first_information_schema_table[] = "columns";
    static const uint8_t first_information_schema_column[] = "table_schema";
    static const uint8_t integer_bytes[] = {0, 0, 0, 42};
    static const uint8_t inserted_marker[] = "insert-visible";
    static const uint8_t updated_marker[] = "update-visible";
    uint8_t planner_response[1024] = {0};
    uint8_t executor_response[1024] = {0};
    uint8_t batch_response[4096] = {0};
    uint8_t grouped_materialized[128] = {0};
    uint8_t grouped_result[256] = {0};
    uint8_t composite_grouped_result[256] = {0};
    uint8_t join_result[128] = {0};
    uint8_t insert_request[256] = {0};
    uint8_t multi_insert_request[256] = {0};
    uint8_t update_request[256] = {0};
    uint8_t inserted_handle[16] = {0};
    uint8_t update_targets[PRDB_SDK_SQL_EXEC_MAX_MUTATION_TARGETS * 16] = {0};
    prdb_sdk_sql_column_shape_v2 planner_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[9] = {{0}};
    uint8_t join_left_planner_response[1024] = {0};
    uint8_t join_right_planner_response[1024] = {0};
    uint8_t join_left_executor_response[1024] = {0};
    uint8_t join_right_executor_response[1024] = {0};
    prdb_sdk_sql_column_shape_v2 join_left_planner_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 join_right_planner_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 join_left_executor_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 join_right_executor_columns[9] = {{0}};
    static prdb_sdk_sql_join_materialized_row_v8 join_left_rows[128];
    static prdb_sdk_sql_join_materialized_row_v8 join_right_rows[128];
    static uint8_t join_materialized_bytes[
        PRDB_SDK_SQL_EXEC_V8_MAX_MATERIALIZED_BYTES];
    uint8_t composite_join_left_planner_response[1024] = {0};
    uint8_t composite_join_right_planner_response[1024] = {0};
    uint8_t composite_join_left_executor_response[1024] = {0};
    uint8_t composite_join_right_executor_response[1024] = {0};
    static uint8_t composite_join_optimizer_request[
        PRDB_SDK_OPTIMIZER_MAX_REQUEST_BYTES];
    static uint8_t composite_join_optimizer_response[
        PRDB_SDK_OPTIMIZER_MAX_PLAN_BYTES + 64];
    static uint8_t group_count_optimizer_request[
        PRDB_SDK_OPTIMIZER_MAX_REQUEST_BYTES];
    static uint8_t group_count_optimizer_response[
        PRDB_SDK_OPTIMIZER_MAX_PLAN_BYTES + 64];
    prdb_sdk_sql_column_shape_v2 composite_join_left_planner_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 composite_join_right_planner_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 composite_join_left_executor_columns[9] = {{0}};
    prdb_sdk_sql_column_shape_v2 composite_join_right_executor_columns[9] = {{0}};
    static prdb_sdk_sql_join_materialized_row_v16 composite_join_left_rows[128];
    static prdb_sdk_sql_join_materialized_row_v16 composite_join_right_rows[128];
    static uint8_t composite_join_materialized_bytes[
        PRDB_SDK_SQL_EXEC_V8_MAX_MATERIALIZED_BYTES];
    prdb_sdk_executor_insert_value_v1 insert_values[1] = {{0}};
    prdb_sdk_executor_insert_value_v1 multi_insert_values[2] = {{0}};
    prdb_sdk_executor_update_patch_v1 update_patches[1] = {{0}};
    prdb_sdk_select_binding_v2 binding = {0};
    prdb_sdk_value_v1 cell_scratch[9] = {{0}};
    prdb_sdk_value_v1 parameter = {0};
    prdb_sdk_value_v1 multi_insert_parameters[2] = {{0}};
    prdb_sdk_value_v1 metadata_parameter = {0};
    prdb_sdk_value_v1 records_table_parameter = {0};
    prdb_sdk_value_v1 records_metadata_parameters[2] = {{0}};
    prdb_sdk_value_v1 private_table_parameter = {0};
    prdb_sdk_value_v1 private_metadata_parameters[2] = {{0}};
    static const uint8_t count_parameter_bytes[8] = {0, 0, 0, 0, 0, 0, 0, 1};
    prdb_sdk_value_v1 count_parameter = {
        sizeof(count_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        count_parameter_bytes, sizeof(count_parameter_bytes)};
    result_context results = {0};
    static const uint8_t id_column_name[] = "id";
    prdb_sdk_sql_column_shape_v2 grouped_column = {
        id_column_name, sizeof(id_column_name) - 1, 0,
        PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0};
    grouped_result_context grouped_results = {
        host, &grouped_column, &grouped_column, grouped_result,
        sizeof(grouped_result), 0, 0};
    composite_grouped_result_context composite_grouped_results = {
        host, executor_columns, 2, composite_grouped_result,
        sizeof(composite_grouped_result), 0, 0};
    joined_result_context joined_results = {
        host, join_left_executor_columns, join_right_executor_columns,
        join_result, sizeof(join_result), 0, 0};
    uint32_t updated_count = 0;
    uint32_t deleted_count = 0;
    uint32_t multi_inserted_count = UINT32_MAX;
    int32_t status;

    if (host == NULL || invocation == NULL ||
        host->struct_size < sizeof(*host) ||
        host->abi_version != PRDB_QUERY_ABI_V3_VERSION ||
        host->executor_request == NULL || host->planner_request == NULL ||
        host->write_result == NULL || host->statement_failure == NULL) {
        return 1;
    }
    if (invocation->parameters_len == 1 && invocation->parameters != NULL &&
        invocation->parameters[0] == 'O') {
        uint8_t output_chunk[PRDB_QUERY_MAX_RESULT_CHUNK_BYTES];
        memset(output_chunk, 0xa5, sizeof(output_chunk));
        for (uint32_t index = 0; index < 256; ++index) {
            if (host->write_result(host->context, output_chunk,
                                   sizeof(output_chunk)) !=
                PRDB_QUERY_CALLBACK_OK) {
                return 1;
            }
        }
        return 0;
    }
    if (invocation->parameters_len == 1 && invocation->parameters != NULL &&
        invocation->parameters[0] == 'C') {
        static const uint8_t update_sql[] =
            "UPDATE grouped_records SET left_key = $2 WHERE left_key = $1";
        static const uint8_t old_key_bytes[] = {0, 0, 0, 4};
        static const uint8_t new_key_bytes[] = {0, 0, 0, 5};
        static const uint8_t confirmed[] = "CANCEL_UPDATE_CONFIRMED";
        prdb_sdk_update_v1 update = {0};
        prdb_sdk_value_v1 parameters[2] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             old_key_bytes, sizeof(old_key_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             new_key_bytes, sizeof(new_key_bytes)}};
        uint8_t request[4096] = {0};
        uint8_t response[4096] = {0};
        uint8_t relation_response[4096] = {0};
        uint8_t scan_response[4096] = {0};
        uint8_t batch_response[4096] = {0};
        uint8_t target_handles[32] = {0};
        prdb_sdk_executor_relation_v1 relation = {0};
        prdb_sdk_sql_column_shape_v2 columns[2] = {{0}};
        prdb_sdk_select_binding_v2 binding = {0};
        prdb_sdk_value_v1 scratch[2] = {{0}};
        prdb_host_api_v5 cancel_host;
        uint32_t updated = UINT32_MAX;
        int32_t update_status;

        if (optimizer_host_v5 == NULL ||
            optimizer_host_v5->optimizer_request == NULL ||
            prdb_sdk_sql_parse_update_v1(update_sql, sizeof(update_sql) - 1,
                                         &update, sizeof(update), NULL) !=
                PRDB_SDK_SQL_OK) {
            return 1;
        }
        cancel_host = *optimizer_host_v5;
        cancel_update_original_host = optimizer_host_v5;
        cancel_update_ready_sent = 0;
        cancel_host.v4.v3.executor_request = cancel_update_executor_request;
        update_status = prdb_sdk_optimizer_execute_update_ast_v1(
            &cancel_host, &update, parameters, 2, schema, sizeof(schema) - 1,
            16, sizeof(batch_response), request, sizeof(request), response,
            sizeof(response), relation_response, sizeof(relation_response),
            scan_response, sizeof(scan_response), batch_response,
            sizeof(batch_response), &relation, sizeof(relation), columns, 2,
            &binding, sizeof(binding), scratch, 2, target_handles, 2, &updated);
        cancel_update_original_host = NULL;
        if (update_status != PRDB_SDK_OPTIMIZER_PLAN_CANCELLED || updated != 0 ||
            cancel_update_ready_sent == 0 ||
            host->write_result(host->context, confirmed,
                               sizeof(confirmed) - 1) != 0) {
            return 1;
        }
        return 0;
    }
    if (invocation->parameters_len == 1 && invocation->parameters != NULL &&
        invocation->parameters[0] == 'G') {
        static const uint8_t update_sql[] =
            "UPDATE grouped_records SET left_key = $2 WHERE left_key = $1";
        static const uint8_t old_key_bytes[] = {0, 0, 0, 4};
        static const uint8_t new_key_bytes[] = {0, 0, 0, 6};
        static const uint8_t ready[] = "UPDATE_STAGE_FAILURE_READY";
        prdb_sdk_update_v1 update = {0};
        prdb_sdk_value_v1 parameters[2] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             old_key_bytes, sizeof(old_key_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             new_key_bytes, sizeof(new_key_bytes)}};
        uint8_t request[4096] = {0};
        uint8_t response[4096] = {0};
        uint8_t relation_response[4096] = {0};
        uint8_t scan_response[4096] = {0};
        uint8_t batch_response[4096] = {0};
        uint8_t target_handles[32] = {0};
        prdb_sdk_executor_relation_v1 relation = {0};
        prdb_sdk_sql_column_shape_v2 columns[2] = {{0}};
        prdb_sdk_select_binding_v2 binding = {0};
        prdb_sdk_value_v1 scratch[2] = {{0}};
        prdb_host_api_v5 failure_host;
        uint32_t updated = UINT32_MAX;
        int32_t update_status;

        if (optimizer_host_v5 == NULL ||
            optimizer_host_v5->optimizer_request == NULL ||
            prdb_sdk_sql_parse_update_v1(update_sql, sizeof(update_sql) - 1,
                                         &update, sizeof(update), NULL) !=
                PRDB_SDK_SQL_OK ||
            host->write_result(host->context, ready, sizeof(ready) - 1) != 0) {
            return 1;
        }
        failure_host = *optimizer_host_v5;
        cancel_update_original_host = optimizer_host_v5;
        cancel_update_ready_sent = 1;
        fail_next_stage_update = 1;
        failure_host.v4.v3.executor_request = cancel_update_executor_request;
        update_status = prdb_sdk_optimizer_execute_update_ast_v1(
            &failure_host, &update, parameters, 2, schema, sizeof(schema) - 1,
            16, sizeof(batch_response), request, sizeof(request), response,
            sizeof(response), relation_response, sizeof(relation_response),
            scan_response, sizeof(scan_response), batch_response,
            sizeof(batch_response), &relation, sizeof(relation), columns, 2,
            &binding, sizeof(binding), scratch, 2, target_handles, 2, &updated);
        cancel_update_original_host = NULL;
        if (update_status != PRDB_SDK_OPTIMIZER_PLAN_CALLBACK_ERROR ||
            updated != 0 || fail_next_stage_update != 0) {
            static const uint8_t message[] =
                "fixture-error-update-stage-failure";
            return report_failure(host, message, sizeof(message) - 1);
        }
        return 0;
    }
    if (invocation->parameters_len == 1 && invocation->parameters != NULL &&
        invocation->parameters[0] == 'T') {
        static const uint8_t target_limit_sql[] =
            "UPDATE grouped_records SET left_key = $2 WHERE left_key = $1";
        static const uint8_t old_key_bytes[] = {0, 0, 0, 4};
        static const uint8_t new_key_bytes[] = {0, 0, 0, 5};
        static const uint8_t ready[] = "UPDATE_TARGET_LIMIT_READY";
        prdb_sdk_update_v1 update = {0};
        prdb_sdk_value_v1 parameters[2] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             old_key_bytes, sizeof(old_key_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             new_key_bytes, sizeof(new_key_bytes)}};
        uint8_t request[4096] = {0};
        uint8_t response[4096] = {0};
        uint8_t relation_response[4096] = {0};
        uint8_t scan_response[4096] = {0};
        uint8_t batch_response[4096] = {0};
        uint8_t target_handles[16] = {0};
        prdb_sdk_executor_relation_v1 relation = {0};
        prdb_sdk_sql_column_shape_v2 columns[2] = {{0}};
        prdb_sdk_select_binding_v2 binding = {0};
        prdb_sdk_value_v1 scratch[2] = {{0}};
        uint32_t updated = UINT32_MAX;
        int32_t update_status;

        if (optimizer_host_v5 == NULL || optimizer_host_v5->optimizer_request == NULL ||
            prdb_sdk_sql_parse_update_v1(
                target_limit_sql, sizeof(target_limit_sql) - 1, &update,
                sizeof(update), NULL) != PRDB_SDK_SQL_OK ||
            host->write_result(host->context, ready, sizeof(ready) - 1) != 0) {
            return 1;
        }
        update_status = prdb_sdk_optimizer_execute_update_ast_v1(
            optimizer_host_v5, &update, parameters, 2, schema,
            sizeof(schema) - 1, 16, sizeof(batch_response), request,
            sizeof(request), response, sizeof(response), relation_response,
            sizeof(relation_response), scan_response, sizeof(scan_response),
            batch_response, sizeof(batch_response), &relation, sizeof(relation),
            columns, 2, &binding, sizeof(binding), scratch, 2, target_handles,
            1, &updated);
        if (update_status != PRDB_SDK_OPTIMIZER_PLAN_REQUEST_LIMIT ||
            updated != 0) {
            static const uint8_t message[] =
                "fixture-error-update-target-limit";
            return report_failure(host, message, sizeof(message) - 1);
        }
        return 0;
    }
    if (invocation->parameters_len == 1 && invocation->parameters != NULL &&
        invocation->parameters[0] == 'F') {
        static const uint8_t denied_update_sql[] =
            "UPDATE linked_records SET id = $2 WHERE id = $1";
        static const uint8_t old_id_bytes[] = {0, 0, 0, 42};
        static const uint8_t new_id_bytes[] = {0, 0, 0, 99};
        prdb_sdk_update_v1 denied_update = {0};
        prdb_sdk_value_v1 denied_parameters[2] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             old_id_bytes, sizeof(old_id_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             new_id_bytes, sizeof(new_id_bytes)}};
        uint8_t request[4096] = {0};
        uint8_t response[4096] = {0};
        uint8_t relation_response[4096] = {0};
        uint8_t scan_response[4096] = {0};
        uint8_t batch_response[4096] = {0};
        uint8_t targets[PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS * 16] = {0};
        prdb_sdk_executor_relation_v1 relation = {0};
        prdb_sdk_sql_column_shape_v2 columns[2] = {{0}};
        prdb_sdk_select_binding_v2 binding = {0};
        prdb_sdk_value_v1 scratch[2] = {{0}};
        uint32_t updated = 0;
        int32_t update_status;

        if (optimizer_host_v5 == NULL || optimizer_host_v5->optimizer_request == NULL ||
            prdb_sdk_sql_parse_update_v1(
                denied_update_sql, sizeof(denied_update_sql) - 1,
                &denied_update, sizeof(denied_update), NULL) != PRDB_SDK_SQL_OK) {
            return 1;
        }
        {
            static const uint8_t ready[] = "UPDATE_REJECTION_READY";
            if (host->write_result(host->context, ready, sizeof(ready) - 1) != 0) {
                return 1;
            }
        }
        update_status = prdb_sdk_optimizer_execute_update_ast_v1(
            optimizer_host_v5, &denied_update, denied_parameters, 2,
            schema, sizeof(schema) - 1, 16, sizeof(batch_response),
            request, sizeof(request), response, sizeof(response),
            relation_response, sizeof(relation_response), scan_response,
            sizeof(scan_response), batch_response, sizeof(batch_response),
            &relation, sizeof(relation), columns, 2, &binding, sizeof(binding),
            scratch, 2, targets, PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
            &updated);
        if (update_status == PRDB_SDK_OPTIMIZER_PLAN_OK || updated != 0) {
            static const uint8_t message[] = "fixture-error-update-was-not-denied";
            return report_failure(host, message, sizeof(message) - 1);
        }
        return 1;
    }
    results.host = host;
    parameter.struct_size = sizeof(parameter);
    parameter.abi_version = PRDB_SDK_VALUE_ABI_V1_VERSION;
    parameter.state = PRDB_SDK_VALUE_STATE_VALUE;
    parameter.type_code = PRDB_SDK_VALUE_TYPE_INTEGER;
    parameter.data = integer_bytes;
    parameter.data_len = sizeof(integer_bytes);
    metadata_parameter.struct_size = sizeof(metadata_parameter);
    metadata_parameter.abi_version = PRDB_SDK_VALUE_ABI_V1_VERSION;
    metadata_parameter.state = PRDB_SDK_VALUE_STATE_VALUE;
    metadata_parameter.type_code = PRDB_SDK_VALUE_TYPE_TEXT;
    metadata_parameter.data = information_schema;
    metadata_parameter.data_len = sizeof(information_schema) - 1;
    records_table_parameter.struct_size = sizeof(records_table_parameter);
    records_table_parameter.abi_version = PRDB_SDK_VALUE_ABI_V1_VERSION;
    records_table_parameter.state = PRDB_SDK_VALUE_STATE_VALUE;
    records_table_parameter.type_code = PRDB_SDK_VALUE_TYPE_TEXT;
    records_table_parameter.data = records_table_name;
    records_table_parameter.data_len = sizeof(records_table_name) - 1;
    private_table_parameter.struct_size = sizeof(private_table_parameter);
    private_table_parameter.abi_version = PRDB_SDK_VALUE_ABI_V1_VERSION;
    private_table_parameter.state = PRDB_SDK_VALUE_STATE_VALUE;
    private_table_parameter.type_code = PRDB_SDK_VALUE_TYPE_TEXT;
    private_table_parameter.data = private_table_name;
    private_table_parameter.data_len = sizeof(private_table_name) - 1;

    status = prdb_sdk_sql_execute_insert_v1(
        host, schema, sizeof(schema) - 1, insert_sql, sizeof(insert_sql) - 1,
        NULL, 0, planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), insert_request, sizeof(insert_request),
        planner_columns, 1, executor_columns, 1, insert_values, 1,
        inserted_handle);
    if (status != PRDB_SDK_SQL_EXEC_OK) {
        static const uint8_t planner_message[] = "fixture-error-insert-planner";
        static const uint8_t executor_message[] = "fixture-error-insert-executor";
        static const uint8_t shape_message[] = "fixture-error-insert-shape";
        static const uint8_t bind_message[] = "fixture-error-insert-bind";
        if (status == PRDB_SDK_SQL_EXEC_PLANNER_ERROR) {
            return report_failure(host, planner_message,
                                  sizeof(planner_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR) {
            return report_failure(host, executor_message,
                                  sizeof(executor_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_SHAPE_MISMATCH) {
            return report_failure(host, shape_message,
                                  sizeof(shape_message) - 1);
        }
        return report_failure(host, bind_message, sizeof(bind_message) - 1);
    }

    results.expected_state = PRDB_SDK_VALUE_STATE_NULL;
    results.marker = inserted_marker;
    results.marker_len = sizeof(inserted_marker) - 1;
    status = prdb_sdk_sql_execute_select_v2(
        host, schema, sizeof(schema) - 1, select_null_sql,
        sizeof(select_null_sql) - 1,
        NULL, 0, 16, sizeof(batch_response), planner_response,
        sizeof(planner_response), executor_response, sizeof(executor_response),
        batch_response, sizeof(batch_response), planner_columns, 1,
        executor_columns, 1, cell_scratch, 1, verify_selected_value, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-null-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_state = PRDB_SDK_VALUE_STATE_VALUE;
    results.expected_value = information_schema;
    results.expected_value_len = sizeof(information_schema) - 1;
    results.marker = NULL;
    results.marker_len = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_schemata_sql, sizeof(select_schemata_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-schemata-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_value = first_information_schema_table;
    results.expected_value_len = sizeof(first_information_schema_table) - 1;
    results.expected_ordinal = 1;
    results.marker = NULL;
    results.marker_len = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_tables_sql, sizeof(select_tables_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK) {
        static const uint8_t planner_message[] = "fixture-error-tables-planner";
        static const uint8_t executor_message[] = "fixture-error-tables-executor";
        static const uint8_t shape_message[] = "fixture-error-tables-shape";
        static const uint8_t bind_message[] = "fixture-error-tables-bind";
        static const uint8_t evaluation_message[] = "fixture-error-tables-evaluation";
        static const uint8_t result_message[] = "fixture-error-tables-result";
        static const uint8_t request_message[] = "fixture-error-tables-request";
        static const uint8_t materialization_message[] =
            "fixture-error-tables-materialization";
        if (status == PRDB_SDK_SQL_EXEC_PLANNER_ERROR) {
            return report_failure(host, planner_message, sizeof(planner_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR) {
            return report_failure(host, executor_message, sizeof(executor_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_SHAPE_MISMATCH) {
            return report_failure(host, shape_message, sizeof(shape_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_BIND_ERROR) {
            return report_failure(host, bind_message, sizeof(bind_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_EVALUATION_ERROR) {
            return report_failure(host, evaluation_message, sizeof(evaluation_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_RESULT_ERROR) {
            return report_failure(host, result_message, sizeof(result_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_REQUEST_LIMIT) {
            return report_failure(host, request_message, sizeof(request_message) - 1);
        }
        if (status == PRDB_SDK_SQL_EXEC_MATERIALIZATION_LIMIT) {
            return report_failure(host, materialization_message,
                                  sizeof(materialization_message) - 1);
        }
        static const uint8_t message[] = "fixture-error-tables-status";
        return report_failure(host, message, sizeof(message) - 1);
    }
    if (results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-tables-count";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_value = first_information_schema_column;
    results.expected_value_len = sizeof(first_information_schema_column) - 1;
    results.expected_ordinal = 2;
    results.marker = NULL;
    results.marker_len = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_columns_sql, sizeof(select_columns_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t header_message[] = "fixture-error-columns-header";
        static const uint8_t cells_message[] = "fixture-error-columns-cells";
        static const uint8_t value_message[] = "fixture-error-columns-value";
        static const uint8_t status_message[] = "fixture-error-columns-status";
        static const uint8_t count_message[] = "fixture-error-columns-count";
        if (results.failure_code == 1) {
            return report_failure(host, header_message, sizeof(header_message) - 1);
        }
        if (results.failure_code == 2) {
            return report_failure(host, cells_message, sizeof(cells_message) - 1);
        }
        if (results.failure_code == 3) {
            return report_failure(host, value_message, sizeof(value_message) - 1);
        }
        if (status != PRDB_SDK_SQL_EXEC_OK) {
            return report_failure(host, status_message, sizeof(status_message) - 1);
        }
        return report_failure(host, count_message, sizeof(count_message) - 1);
    }

    metadata_parameter.data = schema;
    metadata_parameter.data_len = sizeof(schema) - 1;
    records_metadata_parameters[0] = metadata_parameter;
    records_metadata_parameters[1] = records_table_parameter;
    results.expected_value = schema;
    results.expected_value_len = sizeof(schema) - 1;
    results.expected_ordinal = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_schemata_sql, sizeof(select_schemata_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-user-schema-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_value = NULL;
    results.expected_value_len = 0;
    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_tables_sql, sizeof(select_tables_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-user-table-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_value = expected_user_column;
    results.expected_value_len = sizeof(expected_user_column) - 1;
    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_records_columns_sql, sizeof(select_records_columns_sql) - 1,
        records_metadata_parameters, 2, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-user-column-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    metadata_parameter.data = empty_schema;
    metadata_parameter.data_len = sizeof(empty_schema) - 1;
    results.expected_value = empty_schema;
    results.expected_value_len = sizeof(empty_schema) - 1;
    results.expected_ordinal = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_schemata_sql, sizeof(select_schemata_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-empty-schema-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_value = NULL;
    results.expected_value_len = 0;
    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_tables_sql, sizeof(select_tables_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-empty-schema-tables";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_columns_sql, sizeof(select_columns_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-empty-schema-columns";
        return report_failure(host, message, sizeof(message) - 1);
    }

    metadata_parameter.data = schema;
    metadata_parameter.data_len = sizeof(schema) - 1;
    private_metadata_parameters[0] = metadata_parameter;
    private_metadata_parameters[1] = private_table_parameter;
    results.expected_value = private_table_name;
    results.expected_value_len = sizeof(private_table_name) - 1;
    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_private_table_sql, sizeof(select_private_table_sql) - 1,
        private_metadata_parameters, 2, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-private-table-leaked";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_private_columns_sql, sizeof(select_private_columns_sql) - 1,
        private_metadata_parameters, 2, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-private-columns-leaked";
        return report_failure(host, message, sizeof(message) - 1);
    }

    private_table_parameter.data = missing_table_name;
    private_table_parameter.data_len = sizeof(missing_table_name) - 1;
    private_metadata_parameters[1] = private_table_parameter;
    results.expected_value = missing_table_name;
    results.expected_value_len = sizeof(missing_table_name) - 1;
    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_private_table_sql, sizeof(select_private_table_sql) - 1,
        private_metadata_parameters, 2, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-missing-table-result";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_private_columns_sql, sizeof(select_private_columns_sql) - 1,
        private_metadata_parameters, 2, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-missing-columns-result";
        return report_failure(host, message, sizeof(message) - 1);
    }

    metadata_parameter.data = hidden_schema_name;
    metadata_parameter.data_len = sizeof(hidden_schema_name) - 1;
    results.expected_value = NULL;
    results.expected_value_len = 0;
    results.expected_ordinal = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_schemata_sql, sizeof(select_schemata_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-hidden-schema-visible";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_tables_sql, sizeof(select_tables_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-hidden-schema-tables-visible";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_columns_sql, sizeof(select_columns_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-hidden-schema-columns-visible";
        return report_failure(host, message, sizeof(message) - 1);
    }

    metadata_parameter.data = missing_schema_name;
    metadata_parameter.data_len = sizeof(missing_schema_name) - 1;
    results.expected_ordinal = 0;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_schemata_sql, sizeof(select_schemata_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-missing-schema-result";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_tables_sql, sizeof(select_tables_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-missing-schema-tables-result";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_ordinal = 2;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, information_schema, sizeof(information_schema) - 1,
        select_columns_sql, sizeof(select_columns_sql) - 1,
        &metadata_parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, verify_information_schema_name, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-missing-schema-columns-result";
        return report_failure(host, message, sizeof(message) - 1);
    }

    status = prdb_sdk_sql_execute_update_v1(
        host, schema, sizeof(schema) - 1, update_sql, sizeof(update_sql) - 1,
        &parameter, 1, 16, sizeof(batch_response), planner_response,
        sizeof(planner_response), executor_response, sizeof(executor_response),
        batch_response, sizeof(batch_response), update_request,
        sizeof(update_request), planner_columns, 1, executor_columns, 1,
        &binding, sizeof(binding), cell_scratch, 1, update_targets,
        PRDB_SDK_SQL_EXEC_MAX_MUTATION_TARGETS, update_patches, 1,
        &updated_count);
    if (status != PRDB_SDK_SQL_EXEC_OK || updated_count != 1) {
        static const uint8_t message[] = "fixture-error-update";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.expected_state = PRDB_SDK_VALUE_STATE_VALUE;
    results.expected_value = integer_bytes;
    results.expected_value_len = sizeof(integer_bytes);
    results.marker = updated_marker;
    results.marker_len = sizeof(updated_marker) - 1;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, schema, sizeof(schema) - 1, select_value_sql,
        sizeof(select_value_sql) - 1, &parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_selected_value, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-value-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    status = prdb_sdk_sql_execute_group_v5(
        host, schema, sizeof(schema) - 1, select_grouped_min_sql,
        sizeof(select_grouped_min_sql) - 1, NULL, 0, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, grouped_materialized, sizeof(grouped_materialized),
        write_grouped_min, &grouped_results);
    if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-grouped-min";
        return report_failure(host, message, sizeof(message) - 1);
    }

    if (optimizer_host_v5 != NULL) {
        grouped_results.row_count = 0;
        grouped_results.output_len = 0;
        status = prdb_sdk_sql_execute_group_count_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_optimizer_group_count_sql,
            sizeof(select_optimizer_group_count_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), group_count_optimizer_request,
            sizeof(group_count_optimizer_request),
            group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_optimizer_group_count,
            &grouped_results);
        if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 1) {
            static const uint8_t message[] = "fixture-error-optimizer-group-count";
            return report_failure(host, message, sizeof(message) - 1);
        }
    }

    composite_grouped_results.row_count = 0;
    composite_grouped_results.output_len = 0;
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_composite_count_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_composite_group_sql,
            sizeof(select_composite_group_sql) - 1, 16,
            sizeof(batch_response), group_count_optimizer_request,
            sizeof(group_count_optimizer_request),
            group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_composite_count,
            &composite_grouped_results);
    } else {
        status = prdb_sdk_sql_execute_group_v13(
            host, schema, sizeof(schema) - 1, select_composite_group_sql,
            sizeof(select_composite_group_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_composite_count,
            &composite_grouped_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK ||
        composite_grouped_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-composite-group";
        return report_failure(host, message, sizeof(message) - 1);
    }

    composite_grouped_results.row_count = 0;
    composite_grouped_results.output_len = 0;
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_ordered_composite_count_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_composite_order_sql, sizeof(select_composite_order_sql) - 1,
            16, sizeof(batch_response), group_count_optimizer_request,
            sizeof(group_count_optimizer_request), group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response, sizeof(batch_response),
            planner_columns, 9, executor_columns, 9, cell_scratch, 9,
            grouped_materialized, sizeof(grouped_materialized),
            write_grouped_composite_count_v15, &composite_grouped_results);
    } else {
        status = prdb_sdk_sql_execute_group_v15(
            host, schema, sizeof(schema) - 1, select_composite_order_sql,
            sizeof(select_composite_order_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response, sizeof(planner_response),
            executor_response, sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized, sizeof(grouped_materialized),
            write_grouped_composite_count_v15, &composite_grouped_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK ||
        composite_grouped_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-composite-order";
        return report_failure(host, message, sizeof(message) - 1);
    }

    composite_grouped_results.row_count = 0;
    composite_grouped_results.output_len = 0;
    status = prdb_sdk_sql_execute_group_v17(
        host, schema, sizeof(schema) - 1, select_composite_literal_sql,
        sizeof(select_composite_literal_sql) - 1, NULL, 0, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, grouped_materialized, sizeof(grouped_materialized),
        write_grouped_composite_count_v17, &composite_grouped_results);
    if (status != PRDB_SDK_SQL_EXEC_OK ||
        composite_grouped_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-composite-literal";
        return report_failure(host, message, sizeof(message) - 1);
    }

    composite_grouped_results.row_count = 0;
    composite_grouped_results.output_len = 0;
    status = prdb_sdk_sql_execute_group_v18(
        host, schema, sizeof(schema) - 1, select_composite_boolean_sql,
        sizeof(select_composite_boolean_sql) - 1, NULL, 0, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 9, executor_columns, 9,
        cell_scratch, 9, grouped_materialized, sizeof(grouped_materialized),
        write_grouped_composite_count_v18, &composite_grouped_results);
    if (status != PRDB_SDK_SQL_EXEC_OK ||
        composite_grouped_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-composite-boolean";
        return report_failure(host, message, sizeof(message) - 1);
    }

    grouped_results.row_count = 0;
    grouped_results.output_len = 0;
    status = prdb_sdk_sql_execute_group_v6(
        host, schema, sizeof(schema) - 1, select_grouped_sum_sql,
        sizeof(select_grouped_sum_sql) - 1, NULL, 0, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, grouped_materialized, sizeof(grouped_materialized),
        write_grouped_sum, &grouped_results);
    if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-grouped-sum";
        return report_failure(host, message, sizeof(message) - 1);
    }

    grouped_results.row_count = 0;
    grouped_results.output_len = 0;
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_group_aggregate_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_grouped_avg_sql, sizeof(select_grouped_avg_sql) - 1,
            16, sizeof(batch_response), group_count_optimizer_request,
            sizeof(group_count_optimizer_request),
            group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_avg,
            &grouped_results);
    } else {
        status = prdb_sdk_sql_execute_group_v7(
            host, schema, sizeof(schema) - 1, select_grouped_avg_sql,
            sizeof(select_grouped_avg_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_avg,
            &grouped_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-grouped-avg";
        return report_failure(host, message, sizeof(message) - 1);
    }

    grouped_results.row_count = 0;
    grouped_results.output_len = 0;
    grouped_results.group_column = &planner_columns[1];
    grouped_results.aggregate_column = &planner_columns[0];
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_group_aggregate_ordered_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_ordered_grouped_avg_sql,
            sizeof(select_ordered_grouped_avg_sql) - 1, 16,
            sizeof(batch_response), group_count_optimizer_request,
            sizeof(group_count_optimizer_request),
            group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_ordered_grouped_avg,
            &grouped_results);
    } else {
        status = prdb_sdk_sql_execute_group_v12(
            host, schema, sizeof(schema) - 1, select_ordered_grouped_avg_sql,
            sizeof(select_ordered_grouped_avg_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 9, executor_columns, 9,
            cell_scratch, 9, grouped_materialized,
            sizeof(grouped_materialized), write_ordered_grouped_avg,
            &grouped_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-optimizer-ordered-avg";
        return report_failure(host, message, sizeof(message) - 1);
    }
    grouped_results.group_column = &grouped_column;
    grouped_results.aggregate_column = &grouped_column;

    grouped_results.row_count = 0;
    grouped_results.output_len = 0;
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_group_count_ordered_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_grouped_order_sql, sizeof(select_grouped_order_sql) - 1,
            NULL, 0, 16, sizeof(batch_response),
            group_count_optimizer_request, sizeof(group_count_optimizer_request),
            group_count_optimizer_response,
            sizeof(group_count_optimizer_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_ordered_count,
            &grouped_results);
    } else {
        status = prdb_sdk_sql_execute_group_v12(
            host, schema, sizeof(schema) - 1, select_grouped_order_sql,
            sizeof(select_grouped_order_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response,
            sizeof(planner_response), executor_response,
            sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, grouped_materialized,
            sizeof(grouped_materialized), write_grouped_ordered_count,
            &grouped_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK || grouped_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-grouped-order";
        return report_failure(host, message, sizeof(message) - 1);
    }

    status = prdb_sdk_sql_execute_inner_join_v8(
        host, schema, sizeof(schema) - 1, select_join_sql,
        sizeof(select_join_sql) - 1, 16, sizeof(batch_response),
        join_left_planner_response, sizeof(join_left_planner_response),
        join_right_planner_response, sizeof(join_right_planner_response),
        join_left_executor_response, sizeof(join_left_executor_response),
        join_right_executor_response, sizeof(join_right_executor_response),
        batch_response, sizeof(batch_response), join_left_planner_columns, 9,
        join_right_planner_columns, 9, join_left_executor_columns, 9,
        join_right_executor_columns, 9, cell_scratch, 9, join_left_rows, 128,
        join_right_rows, 128, join_materialized_bytes,
        sizeof(join_materialized_bytes), write_joined_row, &joined_results);
    if (status != PRDB_SDK_SQL_EXEC_OK || joined_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-inner-join";
        return report_failure(host, message, sizeof(message) - 1);
    }

    joined_results.row_count = 0;
    joined_results.output_len = 0;
    joined_results.left_columns = composite_join_left_executor_columns;
    joined_results.right_columns = composite_join_right_executor_columns;
    if (optimizer_host_v5 != NULL) {
        status = prdb_sdk_sql_execute_inner_join_optimizer_v1(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_composite_join_sql, sizeof(select_composite_join_sql) - 1,
            16, sizeof(batch_response), composite_join_optimizer_request,
            sizeof(composite_join_optimizer_request),
            composite_join_optimizer_response,
            sizeof(composite_join_optimizer_response),
            composite_join_left_planner_response,
            sizeof(composite_join_left_planner_response),
            composite_join_right_planner_response,
            sizeof(composite_join_right_planner_response),
            composite_join_left_executor_response,
            sizeof(composite_join_left_executor_response),
            composite_join_right_executor_response,
            sizeof(composite_join_right_executor_response), batch_response,
            sizeof(batch_response), composite_join_left_planner_columns, 9,
            composite_join_right_planner_columns, 9,
            composite_join_left_executor_columns, 9,
            composite_join_right_executor_columns, 9, cell_scratch, 9,
            composite_join_left_rows, 128, composite_join_right_rows, 128,
            composite_join_materialized_bytes,
            sizeof(composite_join_materialized_bytes),
            write_composite_joined_row_v19, &joined_results);
    } else {
        status = prdb_sdk_sql_execute_inner_join_v19(
            host, schema, sizeof(schema) - 1, select_composite_join_sql,
            sizeof(select_composite_join_sql) - 1, 16, sizeof(batch_response),
            composite_join_left_planner_response,
            sizeof(composite_join_left_planner_response),
            composite_join_right_planner_response,
            sizeof(composite_join_right_planner_response),
            composite_join_left_executor_response,
            sizeof(composite_join_left_executor_response),
            composite_join_right_executor_response,
            sizeof(composite_join_right_executor_response), batch_response,
            sizeof(batch_response), composite_join_left_planner_columns, 9,
            composite_join_right_planner_columns, 9,
            composite_join_left_executor_columns, 9,
            composite_join_right_executor_columns, 9, cell_scratch, 9,
            composite_join_left_rows, 128, composite_join_right_rows, 128,
            composite_join_materialized_bytes,
            sizeof(composite_join_materialized_bytes),
            write_composite_joined_row_v19, &joined_results);
    }
    if (status != PRDB_SDK_SQL_EXEC_OK || joined_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-composite-inner-join";
        return report_failure(host, message, sizeof(message) - 1);
    }

    joined_results.row_count = 0;
    joined_results.output_len = 0;
    joined_results.left_columns = composite_join_right_executor_columns;
    joined_results.right_columns = composite_join_right_executor_columns;
    status = prdb_sdk_sql_execute_inner_join_v19(
        host, schema, sizeof(schema) - 1, select_self_join_sql,
        sizeof(select_self_join_sql) - 1, 16, sizeof(batch_response),
        composite_join_left_planner_response,
        sizeof(composite_join_left_planner_response),
        composite_join_right_planner_response,
        sizeof(composite_join_right_planner_response),
        composite_join_left_executor_response,
        sizeof(composite_join_left_executor_response),
        composite_join_right_executor_response,
        sizeof(composite_join_right_executor_response), batch_response,
        sizeof(batch_response), composite_join_right_planner_columns, 9,
        composite_join_right_planner_columns, 9,
        composite_join_right_executor_columns, 9,
        composite_join_right_executor_columns, 9, cell_scratch, 9,
        composite_join_left_rows, 128, composite_join_right_rows, 128,
        composite_join_materialized_bytes,
        sizeof(composite_join_materialized_bytes),
        write_self_joined_row_v19, &joined_results);
    if (status != PRDB_SDK_SQL_EXEC_OK || joined_results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-self-inner-join";
        return report_failure(host, message, sizeof(message) - 1);
    }

    joined_results.row_count = 0;
    joined_results.output_len = 0;
    joined_results.left_columns = composite_join_left_executor_columns;
    joined_results.right_columns = composite_join_right_executor_columns;
    status = prdb_sdk_sql_execute_inner_join_v20(
        host, schema, sizeof(schema) - 1, select_filtered_composite_join_sql,
        sizeof(select_filtered_composite_join_sql) - 1,
        &filtered_join_parameter, 1, 16, sizeof(batch_response),
        composite_join_left_planner_response,
        sizeof(composite_join_left_planner_response),
        composite_join_right_planner_response,
        sizeof(composite_join_right_planner_response),
        composite_join_left_executor_response,
        sizeof(composite_join_left_executor_response),
        composite_join_right_executor_response,
        sizeof(composite_join_right_executor_response), batch_response,
        sizeof(batch_response), composite_join_left_planner_columns, 9,
        composite_join_right_planner_columns, 9,
        composite_join_left_executor_columns, 9,
        composite_join_right_executor_columns, 9, cell_scratch, 9,
        composite_join_left_rows, 128, composite_join_right_rows, 128,
        composite_join_materialized_bytes,
        sizeof(composite_join_materialized_bytes),
        write_filtered_joined_row_v20, &joined_results);
    if (status != PRDB_SDK_SQL_EXEC_OK || joined_results.row_count != 1) {
        static const uint8_t message[] = "fixture-error-filtered-inner-join";
        return report_failure(host, message, sizeof(message) - 1);
    }

    if (optimizer_host_v5 != NULL) {
        joined_results.row_count = 0;
        joined_results.output_len = 0;
        status = prdb_sdk_sql_execute_inner_join_optimizer_v20(
            optimizer_host_v5, schema, sizeof(schema) - 1,
            select_filtered_composite_join_sql,
            sizeof(select_filtered_composite_join_sql) - 1,
            &filtered_join_parameter, 1, 16, sizeof(batch_response),
            composite_join_optimizer_request,
            sizeof(composite_join_optimizer_request),
            composite_join_optimizer_response,
            sizeof(composite_join_optimizer_response),
            composite_join_left_planner_response,
            sizeof(composite_join_left_planner_response),
            composite_join_right_planner_response,
            sizeof(composite_join_right_planner_response),
            composite_join_left_executor_response,
            sizeof(composite_join_left_executor_response),
            composite_join_right_executor_response,
            sizeof(composite_join_right_executor_response), batch_response,
            sizeof(batch_response), composite_join_left_planner_columns, 9,
            composite_join_right_planner_columns, 9,
            composite_join_left_executor_columns, 9,
            composite_join_right_executor_columns, 9, cell_scratch, 9,
            composite_join_left_rows, 128, composite_join_right_rows, 128,
            composite_join_materialized_bytes,
            sizeof(composite_join_materialized_bytes),
            write_filtered_joined_row_v20, &joined_results);
        if (status != PRDB_SDK_SQL_EXEC_OK || joined_results.row_count != 1) {
            static const uint8_t message[] =
                "fixture-error-optimizer-filtered-inner-join";
            return report_failure(host, message, sizeof(message) - 1);
        }
    }

    if (optimizer_host != NULL) {
        prdb_sdk_select_v2 optimizer_statement = {0};
        uint8_t optimizer_request[4096] = {0};
        uint8_t optimizer_response[4096] = {0};
        uint8_t relation_response[4096] = {0};
        uint8_t scan_response[4096] = {0};
        uint8_t optimizer_batch[4096] = {0};
        uint8_t optimizer_parameter_bytes[4] = {0, 0, 0, 42};
        uint8_t optimizer_upper_parameter_bytes[4] = {0, 0, 0, 2};
        prdb_sdk_value_v1 optimizer_parameter = {
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
            optimizer_parameter_bytes, sizeof(optimizer_parameter_bytes)};
        prdb_sdk_value_v1 optimizer_range_parameters[2] = {0};
        prdb_sdk_executor_relation_v1 optimizer_relation = {0};
        prdb_sdk_sql_column_shape_v2 optimizer_columns[9] = {{0}};
        prdb_sdk_value_v1 optimizer_source_cells[9] = {{0}};
        prdb_sdk_value_v1 optimizer_projected_values[9] = {{0}};
        optimizer_result_context optimizer_results = {0};
        uint32_t optimizer_rows = 0;
        uint32_t parse_error_offset = 0;
        int32_t optimizer_range_status = PRDB_SDK_OPTIMIZER_PLAN_OK;
        optimizer_range_parameters[0] = optimizer_parameter;
        optimizer_range_parameters[1] = optimizer_parameter;
        optimizer_range_parameters[1].data = optimizer_upper_parameter_bytes;
        optimizer_range_parameters[1].data_len =
            sizeof(optimizer_upper_parameter_bytes);
        if (optimizer_host->optimizer_request == NULL ||
            prdb_sdk_sql_parse_select_v2(
                optimizer_select_sql, sizeof(optimizer_select_sql) - 1,
                &optimizer_statement, sizeof(optimizer_statement),
                &parse_error_offset) != PRDB_SDK_SQL_OK ||
            prdb_sdk_optimizer_execute_select_ast_with_parameters_v1(
                optimizer_host, &optimizer_statement, &optimizer_parameter, 1,
                schema, sizeof(schema) - 1, 16, sizeof(optimizer_batch),
                optimizer_request, sizeof(optimizer_request),
                optimizer_response, sizeof(optimizer_response),
                relation_response, sizeof(relation_response), scan_response,
                sizeof(scan_response), optimizer_batch,
                sizeof(optimizer_batch), &optimizer_relation,
                sizeof(optimizer_relation), optimizer_columns, 9,
                optimizer_source_cells, 9, optimizer_projected_values, 9,
                verify_optimizer_row, &optimizer_results, &optimizer_rows) !=
                PRDB_SDK_OPTIMIZER_PLAN_OK ||
            optimizer_rows != 1 || optimizer_results.row_count != 1 ||
            !optimizer_results.matched_expected_value) {
            static const uint8_t message[] = "fixture-error-optimizer-filter";
            return report_failure(host, message, sizeof(message) - 1);
        }

        memset(&optimizer_statement, 0, sizeof(optimizer_statement));
        memset(&optimizer_results, 0, sizeof(optimizer_results));
        optimizer_results.expected_value = 1;
        optimizer_parameter_bytes[3] = 1;
        optimizer_rows = 0;
        if (prdb_sdk_sql_parse_select_v2(
                optimizer_range_select_sql,
                sizeof(optimizer_range_select_sql) - 1,
                &optimizer_statement, sizeof(optimizer_statement),
                &parse_error_offset) != PRDB_SDK_SQL_OK) {
            static const uint8_t message[] = "fixture-error-optimizer-range-parse";
            return report_failure(host, message, sizeof(message) - 1);
        }
        optimizer_range_status =
            prdb_sdk_optimizer_execute_select_ast_with_parameters_v1(
                optimizer_host, &optimizer_statement, optimizer_range_parameters, 2,
                schema, sizeof(schema) - 1, 16, sizeof(optimizer_batch),
                optimizer_request, sizeof(optimizer_request),
                optimizer_response, sizeof(optimizer_response),
                relation_response, sizeof(relation_response), scan_response,
                sizeof(scan_response), optimizer_batch,
                sizeof(optimizer_batch), &optimizer_relation,
                sizeof(optimizer_relation), optimizer_columns, 9,
                optimizer_source_cells, 9, optimizer_projected_values, 9,
                verify_optimizer_row, &optimizer_results, &optimizer_rows);
        if (optimizer_range_status != PRDB_SDK_OPTIMIZER_PLAN_OK) {
            static const uint8_t message[] = "fixture-error-optimizer-range-execute";
            return report_failure(host, message, sizeof(message) - 1);
        }
        if (optimizer_rows != 1 || optimizer_results.row_count != 1 ||
            !optimizer_results.matched_expected_value) {
            static const uint8_t message[] = "fixture-error-optimizer-range-result";
            return report_failure(host, message, sizeof(message) - 1);
        }
        optimizer_parameter_bytes[3] = 42;

        {
            static uint8_t ordered_request[PRDB_SDK_OPTIMIZER_MAX_REQUEST_BYTES];
            static uint8_t ordered_response[PRDB_SDK_OPTIMIZER_MAX_PLAN_BYTES + 64];
            static uint8_t ordered_relation_response[4096];
            static uint8_t ordered_scan_response[4096];
            static uint8_t ordered_batch[4096];
            static uint8_t ordered_materialized[
                PRDB_SDK_SQL_EXEC_V3_MAX_MATERIALIZED_BYTES];
            static uint8_t ordered_record[256];
            prdb_sdk_select_v3 ordered_statement = {0};
            prdb_sdk_executor_relation_v1 ordered_relation = {0};
            prdb_sdk_sql_column_shape_v2 ordered_columns[9] = {{0}};
            prdb_sdk_value_v1 ordered_source_cells[9] = {{0}};
            prdb_sdk_value_v1 ordered_projected_values[1] = {{0}};
            ordered_optimizer_result_context ordered_results = {
                host, ordered_columns, &ordered_relation, ordered_record,
                sizeof(ordered_record), 0, {9, 7}};
            uint32_t ordered_rows = 0;
            uint32_t ordered_parse_error = 0;
            if (prdb_sdk_sql_parse_select_v3(
                    optimizer_ordered_select_sql,
                    sizeof(optimizer_ordered_select_sql) - 1,
                    &ordered_statement, sizeof(ordered_statement),
                    &ordered_parse_error) != PRDB_SDK_SQL_OK ||
                prdb_sdk_optimizer_execute_ordered_select_ast_v1(
                    optimizer_host, &ordered_statement, schema,
                    sizeof(schema) - 1, 16, sizeof(ordered_batch),
                    ordered_request, sizeof(ordered_request),
                    ordered_response, sizeof(ordered_response),
                    ordered_relation_response,
                    sizeof(ordered_relation_response), ordered_scan_response,
                    sizeof(ordered_scan_response), ordered_batch,
                    sizeof(ordered_batch), &ordered_relation,
                    sizeof(ordered_relation), ordered_columns, 9,
                    ordered_source_cells, 9, ordered_projected_values, 1,
                    ordered_materialized, sizeof(ordered_materialized),
                    emit_ordered_optimizer_row, &ordered_results,
                    &ordered_rows) != PRDB_SDK_OPTIMIZER_PLAN_OK ||
                ordered_rows != 2 || ordered_results.row_count != 2) {
                static const uint8_t message[] =
                    "fixture-error-optimizer-ordered-select";
                return report_failure(host, message, sizeof(message) - 1);
            }
        }
    }

    status = prdb_sdk_sql_execute_delete_v1(
        host, schema, sizeof(schema) - 1, delete_sql, sizeof(delete_sql) - 1,
        &parameter, 1, 16, sizeof(batch_response), planner_response,
        sizeof(planner_response), executor_response, sizeof(executor_response),
        batch_response, sizeof(batch_response), planner_columns, 1,
        executor_columns, 1, &binding, sizeof(binding), cell_scratch, 1,
        update_targets, PRDB_SDK_SQL_EXEC_MAX_MUTATION_TARGETS, &deleted_count);
    if (status != PRDB_SDK_SQL_EXEC_OK || deleted_count != 1) {
        static const uint8_t message[] = "fixture-error-delete";
        return report_failure(host, message, sizeof(message) - 1);
    }

    results.marker = NULL;
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, schema, sizeof(schema) - 1, select_value_sql,
        sizeof(select_value_sql) - 1, &parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_selected_value, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
        static const uint8_t message[] = "fixture-error-final-select";
        return report_failure(host, message, sizeof(message) - 1);
    }

    multi_insert_values[0].struct_size = sizeof(multi_insert_values[0]);
    multi_insert_values[0].abi_version = PRDB_SDK_EXECUTOR_ABI_V1_VERSION;
    multi_insert_values[0].state = PRDB_SDK_EXECUTOR_INSERT_VALUE;
    multi_insert_values[0].value = parameter;
    multi_insert_values[1] = multi_insert_values[0];
    multi_insert_parameters[0] = parameter;
    multi_insert_parameters[1] = parameter;
    status = prdb_sdk_sql_execute_insert_v2(
        host, schema, sizeof(schema) - 1, multi_insert_sql,
        sizeof(multi_insert_sql) - 1, multi_insert_parameters, 2, planner_response,
        sizeof(planner_response), executor_response,
        sizeof(executor_response), multi_insert_request,
        sizeof(multi_insert_request), planner_columns, 1, executor_columns,
        1, multi_insert_values, 2, &multi_inserted_count);
    if (status != PRDB_SDK_SQL_EXEC_OK || multi_inserted_count != 2) {
        static const uint8_t message[] = "fixture-error-multi-insert";
        return report_failure(host, message, sizeof(message) - 1);
    }
    results.row_count = 0;
    status = prdb_sdk_sql_execute_select_v2(
        host, schema, sizeof(schema) - 1, select_value_sql,
        sizeof(select_value_sql) - 1, &parameter, 1, 16,
        sizeof(batch_response), planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 1, executor_columns, 1,
        cell_scratch, 1, verify_selected_value, &results);
    if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 2) {
        static const uint8_t message[] = "fixture-error-multi-insert-visible";
        return report_failure(host, message, sizeof(message) - 1);
    }
    if (optimizer_host_v5 != NULL) {
        static const uint8_t optimizer_grouped_update_sql[] =
            "UPDATE grouped_records SET left_key = $2, right_key = $3 "
            "WHERE left_key = $1";
        static const uint8_t grouped_match_bytes[4] = {0, 0, 0, 1};
        static const uint8_t grouped_left_bytes[4] = {0, 0, 0, 4};
        static const uint8_t grouped_right_bytes[4] = {0, 0, 0, 9};
        prdb_sdk_value_v1 grouped_parameters[3] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_match_bytes, sizeof(grouped_match_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_left_bytes, sizeof(grouped_left_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_right_bytes, sizeof(grouped_right_bytes)}};
        prdb_sdk_update_v1 grouped_update = {0};
        prdb_sdk_executor_relation_v1 grouped_relation = {0};
        prdb_sdk_sql_column_shape_v2 grouped_columns[2] = {{0}};
        prdb_sdk_select_binding_v2 grouped_binding = {0};
        prdb_sdk_value_v1 grouped_scratch[2] = {{0}};
        uint8_t grouped_request[4096] = {0};
        uint8_t grouped_optimizer_response[4096] = {0};
        uint8_t grouped_relation_response[4096] = {0};
        uint8_t grouped_scan_response[4096] = {0};
        uint8_t grouped_batch_response[4096] = {0};
        uint8_t grouped_targets[
            PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS * 16] = {0};
        uint32_t grouped_update_count = 0;
        uint32_t parse_error_offset = 0;
        if (prdb_sdk_sql_parse_update_v1(
                optimizer_grouped_update_sql,
                sizeof(optimizer_grouped_update_sql) - 1, &grouped_update,
                sizeof(grouped_update), &parse_error_offset) !=
                PRDB_SDK_SQL_OK ||
            prdb_sdk_optimizer_execute_update_ast_v1(
                optimizer_host_v5, &grouped_update, grouped_parameters, 3,
                schema, sizeof(schema) - 1, 16,
                sizeof(grouped_batch_response), grouped_request,
                sizeof(grouped_request), grouped_optimizer_response,
                sizeof(grouped_optimizer_response), grouped_relation_response,
                sizeof(grouped_relation_response), grouped_scan_response,
                sizeof(grouped_scan_response), grouped_batch_response,
                sizeof(grouped_batch_response), &grouped_relation,
                sizeof(grouped_relation), grouped_columns, 2, &grouped_binding,
                sizeof(grouped_binding), grouped_scratch, 2, grouped_targets,
                PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
                &grouped_update_count) != PRDB_SDK_OPTIMIZER_PLAN_OK ||
            grouped_update_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-multiple";
            return report_failure(host, message, sizeof(message) - 1);
        }
        static const uint8_t optimizer_grouped_verify_sql[] =
            "UPDATE grouped_records SET left_key = $2 "
            "WHERE left_key = $1 AND right_key = $3";
        prdb_sdk_value_v1 grouped_verify_parameters[3] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_left_bytes, sizeof(grouped_left_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_left_bytes, sizeof(grouped_left_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             grouped_right_bytes, sizeof(grouped_right_bytes)}};
        uint32_t grouped_verify_count = 0;
        if (prdb_sdk_sql_parse_update_v1(
                optimizer_grouped_verify_sql,
                sizeof(optimizer_grouped_verify_sql) - 1, &grouped_update,
                sizeof(grouped_update), &parse_error_offset) !=
                PRDB_SDK_SQL_OK ||
            prdb_sdk_optimizer_execute_update_ast_v1(
                optimizer_host_v5, &grouped_update, grouped_verify_parameters,
                3, schema, sizeof(schema) - 1, 16,
                sizeof(grouped_batch_response), grouped_request,
                sizeof(grouped_request), grouped_optimizer_response,
                sizeof(grouped_optimizer_response), grouped_relation_response,
                sizeof(grouped_relation_response), grouped_scan_response,
                sizeof(grouped_scan_response), grouped_batch_response,
                sizeof(grouped_batch_response), &grouped_relation,
                sizeof(grouped_relation), grouped_columns, 2,
                &grouped_binding, sizeof(grouped_binding), grouped_scratch, 2,
                grouped_targets, PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
                &grouped_verify_count) != PRDB_SDK_OPTIMIZER_PLAN_OK ||
            grouped_verify_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-multiple-visible";
            return report_failure(host, message, sizeof(message) - 1);
        }

        static const uint8_t optimizer_update_sql[] =
            "UPDATE records SET id = $2 WHERE id = $1";
        static const uint8_t updated_integer_bytes[] = {0, 0, 0, 43};
        prdb_sdk_update_v1 optimizer_update = {0};
        uint8_t optimizer_update_request[4096] = {0};
        uint8_t optimizer_update_response[4096] = {0};
        uint8_t optimizer_relation_response[4096] = {0};
        uint8_t optimizer_scan_response[4096] = {0};
        uint8_t optimizer_batch_response[4096] = {0};
        uint8_t old_parameter_bytes[4] = {0, 0, 0, 42};
        prdb_sdk_value_v1 optimizer_update_parameters[2] = {
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             old_parameter_bytes, sizeof(old_parameter_bytes)},
            {sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
             PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
             updated_integer_bytes, sizeof(updated_integer_bytes)}};
        prdb_sdk_executor_relation_v1 optimizer_update_relation = {0};
        prdb_sdk_sql_column_shape_v2 optimizer_update_columns[1] = {{0}};
        prdb_sdk_select_binding_v2 optimizer_update_binding = {0};
        prdb_sdk_value_v1 optimizer_update_cell_scratch[1] = {{0}};
        uint8_t optimizer_update_targets[
            PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS * 16] = {0};
        uint32_t optimized_update_count = 0;
        int32_t optimized_update_status;

        if (optimizer_host_v5->optimizer_request == NULL ||
            prdb_sdk_sql_parse_update_v1(
                optimizer_update_sql, sizeof(optimizer_update_sql) - 1,
                &optimizer_update, sizeof(optimizer_update), NULL) !=
                PRDB_SDK_SQL_OK) {
            static const uint8_t message[] = "fixture-error-optimizer-update";
            return report_failure(host, message, sizeof(message) - 1);
        }
        optimized_update_status = prdb_sdk_optimizer_execute_update_ast_v1(
            optimizer_host_v5, &optimizer_update, optimizer_update_parameters,
            2, schema, sizeof(schema) - 1, 16,
            sizeof(optimizer_batch_response), optimizer_update_request,
            sizeof(optimizer_update_request), optimizer_update_response,
            sizeof(optimizer_update_response), optimizer_relation_response,
            sizeof(optimizer_relation_response), optimizer_scan_response,
            sizeof(optimizer_scan_response), optimizer_batch_response,
            sizeof(optimizer_batch_response), &optimizer_update_relation,
            sizeof(optimizer_update_relation), optimizer_update_columns, 1,
            &optimizer_update_binding, sizeof(optimizer_update_binding),
            optimizer_update_cell_scratch, 1, optimizer_update_targets,
            PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
            &optimized_update_count);
        if (optimized_update_status != PRDB_SDK_OPTIMIZER_PLAN_OK) {
            static const uint8_t message[] = "fixture-error-optimizer-update";
            return report_failure(host, message, sizeof(message) - 1);
        }
        if (optimized_update_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-count";
            return report_failure(host, message, sizeof(message) - 1);
        }
        parameter.data = updated_integer_bytes;
        parameter.data_len = sizeof(updated_integer_bytes);
        results.expected_state = PRDB_SDK_VALUE_STATE_VALUE;
        results.expected_value = updated_integer_bytes;
        results.expected_value_len = sizeof(updated_integer_bytes);
        results.marker = NULL;
        results.row_count = 0;
        status = prdb_sdk_sql_execute_select_v2(
            host, schema, sizeof(schema) - 1, select_value_sql,
            sizeof(select_value_sql) - 1, &parameter, 1, 16,
            sizeof(batch_response), planner_response, sizeof(planner_response),
            executor_response, sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, verify_selected_value, &results);
        if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-visible";
            return report_failure(host, message, sizeof(message) - 1);
        }

        static const uint8_t optimizer_null_update_sql[] =
            "UPDATE records SET id = NULL WHERE id = $1";
        prdb_sdk_value_v1 optimizer_null_parameter = {
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
            updated_integer_bytes, sizeof(updated_integer_bytes)};
        uint32_t optimized_null_update_count = 0;
        if (prdb_sdk_sql_parse_update_v1(
                optimizer_null_update_sql,
                sizeof(optimizer_null_update_sql) - 1, &optimizer_update,
                sizeof(optimizer_update), NULL) != PRDB_SDK_SQL_OK ||
            prdb_sdk_optimizer_execute_update_ast_v1(
                optimizer_host_v5, &optimizer_update, &optimizer_null_parameter,
                1, schema, sizeof(schema) - 1, 16,
                sizeof(optimizer_batch_response), optimizer_update_request,
                sizeof(optimizer_update_request), optimizer_update_response,
                sizeof(optimizer_update_response), optimizer_relation_response,
                sizeof(optimizer_relation_response), optimizer_scan_response,
                sizeof(optimizer_scan_response), optimizer_batch_response,
                sizeof(optimizer_batch_response), &optimizer_update_relation,
                sizeof(optimizer_update_relation), optimizer_update_columns, 1,
                &optimizer_update_binding, sizeof(optimizer_update_binding),
                optimizer_update_cell_scratch, 1, optimizer_update_targets,
                PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
                &optimized_null_update_count) != PRDB_SDK_OPTIMIZER_PLAN_OK ||
            optimized_null_update_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-null";
            return report_failure(host, message, sizeof(message) - 1);
        }
        results.expected_state = PRDB_SDK_VALUE_STATE_NULL;
        results.expected_value = NULL;
        results.expected_value_len = 0;
        results.marker = NULL;
        results.row_count = 0;
        status = prdb_sdk_sql_execute_select_v2(
            host, schema, sizeof(schema) - 1, select_null_sql,
            sizeof(select_null_sql) - 1, NULL, 0, 16,
            sizeof(batch_response), planner_response, sizeof(planner_response),
            executor_response, sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, verify_selected_value, &results);
        if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-null-visible";
            return report_failure(host, message, sizeof(message) - 1);
        }
        status = prdb_sdk_sql_execute_update_v1(
            host, schema, sizeof(schema) - 1, update_sql,
            sizeof(update_sql) - 1, &optimizer_null_parameter, 1, 16,
            sizeof(batch_response), planner_response, sizeof(planner_response),
            executor_response, sizeof(executor_response), batch_response,
            sizeof(batch_response), update_request, sizeof(update_request),
            planner_columns, 1, executor_columns, 1, &binding,
            sizeof(binding), cell_scratch, 1, update_targets,
            PRDB_SDK_SQL_EXEC_MAX_MUTATION_TARGETS, update_patches, 1,
            &updated_count);
        if (status != PRDB_SDK_SQL_EXEC_OK || updated_count != 2) {
            static const uint8_t message[] =
                "fixture-error-optimizer-update-null-restore";
            return report_failure(host, message, sizeof(message) - 1);
        }

        static const uint8_t optimizer_delete_sql[] =
            "DELETE FROM records WHERE id = $1";
        prdb_sdk_delete_v1 optimizer_delete = {0};
        uint8_t optimizer_delete_targets[
            PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS * 16] = {0};
        uint32_t optimized_delete_count = 0;
        if (prdb_sdk_sql_parse_delete_v1(
                optimizer_delete_sql, sizeof(optimizer_delete_sql) - 1,
                &optimizer_delete, sizeof(optimizer_delete), NULL) !=
                PRDB_SDK_SQL_OK ||
            prdb_sdk_optimizer_execute_delete_ast_v1(
                optimizer_host_v5, &optimizer_delete,
                &optimizer_update_parameters[1], 1, schema,
                sizeof(schema) - 1, 16, sizeof(optimizer_batch_response),
                optimizer_update_request, sizeof(optimizer_update_request),
                optimizer_update_response, sizeof(optimizer_update_response),
                optimizer_relation_response, sizeof(optimizer_relation_response),
                optimizer_scan_response, sizeof(optimizer_scan_response),
                optimizer_batch_response, sizeof(optimizer_batch_response),
                &optimizer_update_relation, sizeof(optimizer_update_relation),
                optimizer_update_columns, 1, &optimizer_update_binding,
                sizeof(optimizer_update_binding), optimizer_update_cell_scratch,
                1, optimizer_delete_targets,
                PRDB_SDK_OPTIMIZER_PLAN_MAX_DELETE_TARGETS,
                &optimized_delete_count) != PRDB_SDK_OPTIMIZER_PLAN_OK ||
            optimized_delete_count != 2) {
            static const uint8_t message[] = "fixture-error-optimizer-delete";
            return report_failure(host, message, sizeof(message) - 1);
        }
        results.row_count = 0;
        status = prdb_sdk_sql_execute_select_v2(
            host, schema, sizeof(schema) - 1, select_value_sql,
            sizeof(select_value_sql) - 1, &parameter, 1, 16,
            sizeof(batch_response), planner_response, sizeof(planner_response),
            executor_response, sizeof(executor_response), batch_response,
            sizeof(batch_response), planner_columns, 1, executor_columns, 1,
            cell_scratch, 1, verify_selected_value, &results);
        if (status != PRDB_SDK_SQL_EXEC_OK || results.row_count != 0) {
            static const uint8_t message[] =
                "fixture-error-optimizer-delete-visible";
            return report_failure(host, message, sizeof(message) - 1);
        }
    }
    return 0;
}

int32_t prdb_query_main_v3(const prdb_host_api_v3 *host,
                           const prdb_invocation_v3 *invocation) {
    return run_write_read_fixture(host, invocation, NULL, NULL);
}

int32_t prdb_query_main_v4(const prdb_host_api_v4 *host,
                           const prdb_invocation_v4 *invocation) {
    if (host == NULL || host->v3.struct_size < sizeof(host->v3) ||
        host->v3.abi_version != PRDB_QUERY_ABI_V3_VERSION ||
        host->optimizer_request == NULL) {
        return 1;
    }
    return run_write_read_fixture(&host->v3, invocation, host, NULL);
}

int32_t prdb_query_main_v5(const prdb_host_api_v5 *host,
                           const prdb_invocation_v5 *invocation) {
    if (host == NULL || host->v4.v3.struct_size < sizeof(host->v4.v3) ||
        host->v4.v3.abi_version != PRDB_QUERY_ABI_V3_VERSION ||
        host->v4.optimizer_request == NULL || host->optimizer_request == NULL) {
        return 1;
    }
    return run_write_read_fixture(&host->v4.v3, invocation, &host->v4, host);
}
