#include "prdb_sdk_sql_exec_v10.h"
#include "prdb_sdk_sql_exec_v11.h"
#include "prdb_sdk_sql_exec_v12.h"
#include "prdb_sdk_sql_exec_v13.h"
#include "prdb_sdk_sql_exec_v14.h"
#include "prdb_sdk_sql_exec_v15.h"
#include "prdb_sdk_sql_exec_v17.h"
#include "prdb_sdk_sql_exec_v18.h"
#include "prdb_sdk_sql_exec_v21.h"
#include "prdb_sdk_sql_exec_v22.h"
#include "prdb_sdk_sql_result_v1.h"

#include <assert.h>
#include <string.h>

typedef struct mock_state {
    uint8_t planner[256];
    uint32_t planner_len;
    uint8_t executor[256];
    uint32_t executor_len;
    uint8_t batch[540000];
    uint32_t batch_len;
    uint8_t continued_batch[540000];
    uint32_t continued_batch_len;
    uint32_t batch_finished;
    uint32_t planner_calls;
    uint32_t executor_calls;
    uint32_t executor_requests;
    uint32_t next_calls;
    uint32_t statement_failed;
    uint32_t cancelled;
    uint32_t cancel_after_first_batch;
    uint32_t cancel_when_batch_finishes;
    uint32_t cancel_after_group_callbacks;
    uint32_t streamed_pages;
    uint32_t max_candidates;
    uint32_t text_key_mode;
} mock_state;

typedef struct group_capture {
    mock_state *mock;
    uint32_t calls;
    uint32_t expect_having_literal;
    uint64_t expected_row_count;
    int32_t keys[128];
    int32_t second_keys[128];
    int32_t aggregates[128];
    uint32_t aggregate_states[128];
    uint64_t row_counts[128];
    uint32_t text_key_bytes;
    uint32_t scalar_order_mode;
    uint32_t scalar_order_type;
    uint8_t scalar_order_precision;
    uint8_t scalar_order_scale;
    uint32_t scalar_order_descending;
    const uint8_t *scalar_order_expected[3];
    uint32_t scalar_order_expected_len[3];
    uint8_t scalar_order_sqlr[3][128];
    uint64_t scalar_order_sqlr_len[3];
} group_capture;

static void put_u16(uint8_t *p, uint32_t *offset, uint16_t value) {
    p[(*offset)++] = (uint8_t)(value >> 8);
    p[(*offset)++] = (uint8_t)value;
}

static void put_u32(uint8_t *p, uint32_t *offset, uint32_t value) {
    p[(*offset)++] = (uint8_t)(value >> 24);
    p[(*offset)++] = (uint8_t)(value >> 16);
    p[(*offset)++] = (uint8_t)(value >> 8);
    p[(*offset)++] = (uint8_t)value;
}

static void put_name(uint8_t *p, uint32_t *offset, const char *name) {
    uint16_t length = (uint16_t)strlen(name);
    put_u16(p, offset, length);
    memcpy(p + *offset, name, length);
    *offset += length;
}

static uint32_t make_relation(uint8_t *p, uint8_t handle_kind,
                              uint8_t handle_fill, uint32_t key_type) {
    static const char *names[] = {"id", "score"};
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = 0;
    memset(p + offset, 0x11, 16); offset += 16;
    p[offset++] = handle_kind;
    memset(p + offset, handle_fill, 15); offset += 15;
    put_name(p, &offset, "widgets");
    put_u16(p, &offset, 2);
    for (uint32_t index = 0; index < 2; ++index) {
        memset(p + offset, (int)(0x33 + index), 16); offset += 16;
        put_u32(p, &offset, index);
        p[offset++] = 1;
        p[offset++] = index == 0 ? key_type : PRDB_SDK_VALUE_TYPE_INTEGER;
        if (index == 0 && key_type == PRDB_SDK_VALUE_TYPE_DECIMAL) {
            p[offset++] = 38;
            p[offset++] = 2;
            p[offset++] = 0;
            p[offset++] = 0;
        } else if (index == 0 &&
                   key_type == PRDB_SDK_VALUE_TYPE_VARCHAR) {
            put_u32(p, &offset, 128);
        } else {
            p[offset++] = 0;
            p[offset++] = 0;
            p[offset++] = 0;
            p[offset++] = 0;
        }
        put_name(p, &offset, names[index]);
    }
    return offset;
}

static uint32_t make_decimal_relation(uint8_t *p, uint8_t handle_kind,
                                     uint8_t handle_fill,
                                     uint8_t decimal_precision,
                                     uint8_t decimal_scale) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = 0;
    memset(p + offset, 0x11, 16); offset += 16;
    p[offset++] = handle_kind;
    memset(p + offset, handle_fill, 15); offset += 15;
    put_name(p, &offset, "widgets");
    put_u16(p, &offset, 2);
    for (uint32_t index = 0; index < 2; ++index) {
        memset(p + offset, (int)(0x33 + index), 16); offset += 16;
        put_u32(p, &offset, index);
        p[offset++] = 1;
        p[offset++] = index == 0 ? PRDB_SDK_VALUE_TYPE_INTEGER
                                 : PRDB_SDK_VALUE_TYPE_DECIMAL;
        p[offset++] = index == 0 ? 0 : decimal_precision;
        p[offset++] = index == 0 ? 0 : decimal_scale;
        p[offset++] = 0;
        p[offset++] = 0;
        put_name(p, &offset, index == 0 ? "id" : "score");
    }
    return offset;
}

static void append_integer_cell(uint8_t *p, uint32_t *offset, int32_t value) {
    uint32_t bits = (uint32_t)value;
    p[(*offset)++] = PRDB_SDK_VALUE_STATE_VALUE;
    p[(*offset)++] = PRDB_SDK_VALUE_TYPE_INTEGER;
    put_u32(p, offset, 4);
    put_u32(p, offset, bits);
}

static void append_typed_cell(uint8_t *p, uint32_t *offset, uint32_t type,
                              const uint8_t *bytes, uint32_t byte_count) {
    p[(*offset)++] = PRDB_SDK_VALUE_STATE_VALUE;
    p[(*offset)++] = (uint8_t)type;
    put_u32(p, offset, byte_count);
    memcpy(p + *offset, bytes, byte_count);
    *offset += byte_count;
}

static uint32_t make_typed_key_rows(
    uint8_t *p, uint32_t finished, uint32_t key_type,
    const uint8_t *const key_values[3], const uint32_t key_lengths[3],
    uint32_t row_count) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = (uint8_t)finished;
    if (!finished) {
        p[offset++] = 7;
        memset(p + offset, 0x55, 15);
        offset += 15;
    }
    put_u16(p, &offset, (uint16_t)row_count);
    for (uint32_t index = 0; index < row_count; ++index) {
        p[offset++] = 8;
        memset(p + offset, (int)(index + 1), 15);
        offset += 15;
        put_u16(p, &offset, 2);
        append_typed_cell(p, &offset, key_type, key_values[index],
                          key_lengths[index]);
        append_integer_cell(p, &offset, 1);
    }
    return offset;
}

static void append_decimal_cell(uint8_t *p, uint32_t *offset,
                                int64_t coefficient) {
    uint64_t bits = (uint64_t)coefficient;
    p[(*offset)++] = PRDB_SDK_VALUE_STATE_VALUE;
    p[(*offset)++] = PRDB_SDK_VALUE_TYPE_DECIMAL;
    put_u32(p, offset, 16);
    memset(p + *offset, coefficient < 0 ? 0xff : 0, 16);
    for (uint32_t index = 0; index < 8; ++index) {
        p[*offset + 15 - index] = (uint8_t)bits;
        bits >>= 8;
    }
    *offset += 16;
}

static void append_text_cell(uint8_t *p, uint32_t *offset,
                             uint32_t key_index, uint32_t byte_count) {
    p[(*offset)++] = PRDB_SDK_VALUE_STATE_VALUE;
    p[(*offset)++] = PRDB_SDK_VALUE_TYPE_TEXT;
    put_u32(p, offset, byte_count);
    memset(p + *offset, 'a', byte_count);
    p[*offset + byte_count - 3] = (uint8_t)('0' + key_index / 100);
    p[*offset + byte_count - 2] =
        (uint8_t)('0' + (key_index / 10) % 10);
    p[*offset + byte_count - 1] = (uint8_t)('0' + key_index % 10);
    *offset += byte_count;
}

static uint32_t make_text_rows(uint8_t *p, uint32_t finished,
                               uint32_t first_key, uint16_t row_count,
                               uint32_t key_bytes) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = (uint8_t)finished;
    if (!finished) {
        p[offset++] = 7;
        memset(p + offset, 0x55, 15);
        offset += 15;
    }
    put_u16(p, &offset, row_count);
    for (uint32_t index = 0; index < row_count; ++index) {
        p[offset++] = 8;
        memset(p + offset, (int)(index + 1), 15);
        offset += 15;
        put_u16(p, &offset, 2);
        append_text_cell(p, &offset, first_key + index, key_bytes);
        append_integer_cell(p, &offset, 1);
    }
    return offset;
}

static uint32_t make_rows(uint8_t *p, uint32_t finished,
                          const int32_t *keys, const int32_t *scores,
                          const uint8_t *score_states, uint16_t row_count) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = (uint8_t)finished;
    if (!finished) {
        p[offset++] = 7;
        memset(p + offset, 0x55, 15); offset += 15;
    }
    put_u16(p, &offset, row_count);
    for (uint32_t index = 0; index < row_count; ++index) {
        p[offset++] = 8;
        memset(p + offset, (int)(index + 1), 15); offset += 15;
        put_u16(p, &offset, 2);
        append_integer_cell(p, &offset, keys[index]);
        if (score_states[index] == PRDB_SDK_VALUE_STATE_VALUE) {
            append_integer_cell(p, &offset, scores[index]);
        } else {
            p[offset++] = score_states[index];
        }
    }
    return offset;
}

static uint32_t make_decimal_rows(uint8_t *p, uint32_t finished,
                                 const int32_t *keys,
                                 const int64_t *coefficients,
                                 const uint8_t *score_states,
                                 uint16_t row_count) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = (uint8_t)finished;
    if (!finished) {
        p[offset++] = 7;
        memset(p + offset, 0x55, 15); offset += 15;
    }
    put_u16(p, &offset, row_count);
    for (uint32_t index = 0; index < row_count; ++index) {
        p[offset++] = 8;
        memset(p + offset, (int)(index + 1), 15); offset += 15;
        put_u16(p, &offset, 2);
        append_integer_cell(p, &offset, keys[index]);
        if (score_states == NULL ||
            score_states[index] == PRDB_SDK_VALUE_STATE_VALUE) {
            append_decimal_cell(p, &offset, coefficients[index]);
        } else {
            p[offset++] = score_states[index];
        }
    }
    return offset;
}

static uint32_t make_decimal_rows_bytes(
    uint8_t *p, uint32_t finished, const int32_t *keys,
    const uint8_t (*coefficients)[16], uint16_t row_count) {
    uint32_t offset = 0;
    put_u16(p, &offset, 1);
    p[offset++] = (uint8_t)finished;
    if (!finished) {
        p[offset++] = 7;
        memset(p + offset, 0x55, 15);
        offset += 15;
    }
    put_u16(p, &offset, row_count);
    for (uint32_t index = 0; index < row_count; ++index) {
        p[offset++] = 8;
        memset(p + offset, (int)(index + 1), 15);
        offset += 15;
        put_u16(p, &offset, 2);
        append_integer_cell(p, &offset, keys[index]);
        p[offset++] = PRDB_SDK_VALUE_STATE_VALUE;
        p[offset++] = PRDB_SDK_VALUE_TYPE_DECIMAL;
        put_u32(p, &offset, 16);
        memcpy(p + offset, coefficients[index], 16);
        offset += 16;
    }
    return offset;
}

static int32_t statement_failure(void *context, uint32_t failure_code) {
    mock_state *mock = (mock_state *)context;
    assert(failure_code == PRDB_QUERY_STATEMENT_FAILURE_SQL);
    mock->statement_failed = 1;
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t planner_request(void *context, uint16_t operation,
                               const uint8_t *request, uint64_t request_len,
                               uint8_t *response, uint64_t capacity,
                               uint64_t *response_len) {
    static const uint8_t expected[] = {
        0, 1, 0, 3, 0, 7, 'a', 'p', 'p', 'w', 'i', 'd', 'g', 'e', 't', 's'};
    mock_state *mock = (mock_state *)context;
    assert(operation == PRDB_SDK_PLANNER_OP_DESCRIBE_RELATION);
    assert(request_len == sizeof(expected));
    assert(memcmp(request, expected, sizeof(expected)) == 0);
    assert(mock->planner_len <= capacity);
    memcpy(response, mock->planner, mock->planner_len);
    *response_len = mock->planner_len;
    ++mock->planner_calls;
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t executor_request(void *context, uint16_t operation,
                                const uint8_t *request, uint64_t request_len,
                                uint8_t *response, uint64_t capacity,
                                uint64_t *response_len) {
    mock_state *mock = (mock_state *)context;
    ++mock->executor_requests;
    if (operation == PRDB_SDK_EXECUTOR_OP_CHECK_CANCELLATION) {
        const uint8_t reply[] = {0, 1, (uint8_t)mock->cancelled};
        assert(request_len == 0 && sizeof(reply) <= capacity);
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_RESOLVE_RELATION) {
        static const uint8_t expected[] = {
            0, 1, 0, 3, 0, 7, 'a', 'p', 'p', 'w', 'i', 'd', 'g', 'e', 't', 's'};
        assert(request_len == sizeof(expected));
        assert(memcmp(request, expected, sizeof(expected)) == 0);
        assert(mock->executor_len <= capacity);
        memcpy(response, mock->executor, mock->executor_len);
        *response_len = mock->executor_len;
    } else if (operation == PRDB_SDK_EXECUTOR_OP_OPEN_SCAN) {
        static const uint8_t expected[20] = {
            0, 1, 6, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0, 0};
        static const uint8_t reply[18] = {
            0, 1, 7, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44};
        assert(request_len == sizeof(expected));
        assert(memcmp(request, expected, sizeof(expected)) == 0);
        assert(sizeof(reply) <= capacity);
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else if (operation == PRDB_SDK_EXECUTOR_OP_NEXT_BATCH) {
        static const uint8_t finished_reply[5] = {0, 1, 1, 0, 0};
        static const uint8_t initial_cursor_tail[16] = {
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0};
        static const uint8_t continued_cursor_tail[16] = {
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0};
        assert(request_len == 24);
        assert(request[0] == 0 && request[1] == 1 && request[2] == 7);
        if (mock->streamed_pages != 0 &&
            mock->next_calls < mock->streamed_pages) {
            const uint8_t *batch = mock->batch;
            uint32_t batch_len = mock->batch_len;
            if (mock->next_calls != 0 && mock->continued_batch_len != 0) {
                batch = mock->continued_batch;
                batch_len = mock->continued_batch_len;
            }
            assert(batch_len <= capacity);
            memcpy(response, batch, batch_len);
            *response_len = batch_len;
        } else if (mock->next_calls == 0) {
            assert(memcmp(request + 3, initial_cursor_tail, 16) == 0);
            assert(mock->batch_len <= capacity);
            memcpy(response, mock->batch, mock->batch_len);
            *response_len = mock->batch_len;
        } else {
            assert(memcmp(request + 3, continued_cursor_tail, 16) == 0);
            assert(sizeof(finished_reply) <= capacity);
            memcpy(response, finished_reply, sizeof(finished_reply));
            *response_len = sizeof(finished_reply);
        }
        ++mock->next_calls;
        if (mock->next_calls == 1 && mock->cancel_after_first_batch) {
            mock->cancelled = 1;
        }
        if (mock->cancel_when_batch_finishes && mock->batch_finished) {
            mock->cancelled = 1;
        }
    } else if (operation == PRDB_SDK_EXECUTOR_OP_CLOSE_CURSOR) {
        static const uint8_t reply[] = {0, 1, 0};
        static const uint8_t continued_cursor_tail[15] = {
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55};
        static const uint8_t initial_cursor_tail[15] = {
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44};
        assert(request_len == 18);
        assert(request[0] == 0 && request[1] == 1 && request[2] == 7);
        assert(memcmp(request + 3,
                      request[3] == 0x44 ? initial_cursor_tail
                                          : continued_cursor_tail,
                      15) == 0);
        assert(sizeof(reply) <= capacity);
        memcpy(response, reply, sizeof(reply));
        *response_len = sizeof(reply);
    } else {
        assert(!"unexpected executor operation");
    }
    if (operation != PRDB_SDK_EXECUTOR_OP_CHECK_CANCELLATION) {
        ++mock->executor_calls;
    }
    return PRDB_QUERY_CALLBACK_OK;
}

static int32_t on_group(void *context, const prdb_sdk_value_v1 *group_key,
                        const prdb_sdk_value_v1 *aggregate_value,
                        uint64_t matching_row_count,
                        const prdb_sdk_select_binding_v10 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    int32_t aggregate = 0;
    assert(capture->mock->batch_finished != 0);
    assert(capture->mock->executor_calls == 3);
    assert(capture->calls < 128);
    assert(binding->base.base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_MIN ||
           binding->base.base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_MAX ||
           binding->base.base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_SUM ||
           binding->base.base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_AVG ||
           binding->base.base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR);
    assert(prdb_sdk_value_get_integer_v1(group_key, &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    if (binding->base.base.aggregate_kind ==
        PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR) {
        assert(aggregate_value == NULL);
        aggregate = (int32_t)matching_row_count;
        capture->aggregate_states[capture->calls] =
            PRDB_SDK_VALUE_STATE_VALUE;
    } else {
        assert(aggregate_value != NULL);
        capture->aggregate_states[capture->calls] = aggregate_value->state;
        if (aggregate_value->state == PRDB_SDK_VALUE_STATE_VALUE) {
            if (binding->base.base.aggregate_kind ==
                PRDB_SDK_SQL_V7_AGGREGATE_SUM) {
                int64_t sum = 0;
                assert(prdb_sdk_value_get_bigint_v1(aggregate_value, &sum) ==
                       PRDB_SDK_VALUE_ACCESS_OK);
                aggregate = (int32_t)sum;
                assert(sum == aggregate);
            } else if (binding->base.base.aggregate_kind ==
                       PRDB_SDK_SQL_V7_AGGREGATE_AVG) {
                uint32_t coefficient = 0;
                assert(aggregate_value->type_code ==
                           PRDB_SDK_VALUE_TYPE_DECIMAL &&
                       aggregate_value->data_len == 16);
                for (uint32_t index = 0; index < 12; ++index) {
                    assert(aggregate_value->data[index] == 0);
                }
                for (uint32_t index = 12; index < 16; ++index) {
                    coefficient =
                        (coefficient << 8) | aggregate_value->data[index];
                }
                aggregate = (int32_t)coefficient;
            } else {
                assert(prdb_sdk_value_get_integer_v1(aggregate_value,
                                                     &aggregate) ==
                       PRDB_SDK_VALUE_ACCESS_OK);
            }
        } else {
            assert(aggregate_value->state == PRDB_SDK_VALUE_STATE_NULL);
        }
    }
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = aggregate;
    capture->row_counts[capture->calls] = matching_row_count;
    ++capture->calls;
    if (capture->mock->cancel_after_group_callbacks == capture->calls) {
        capture->mock->cancelled = 1;
    }
    return 0;
}

static int32_t run_query(mock_state *mock, const uint8_t *sql,
                         uint64_t sql_len,
                         const prdb_sdk_value_v1 *parameters,
                         uint32_t parameter_count, uint64_t arena_capacity,
                         group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[64] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v10(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameters,
        parameter_count, 4, 512, planner_response, sizeof(planner_response),
        executor_response, sizeof(executor_response), batch_response,
        sizeof(batch_response), planner_columns, 2, executor_columns, 2,
        cell_scratch, 2, materialized_bytes, arena_capacity, on_group, capture);
}

static int32_t on_group_v11(void *context,
                            const prdb_sdk_value_v1 *group_key,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v11 *binding) {
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V11_VERSION);
    return on_group(context, group_key, aggregate_value, matching_row_count,
                    &binding->base);
}

static int32_t run_query_v11(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len, uint64_t arena_capacity,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[64] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v11(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, arena_capacity, on_group_v11, capture);
}

static uint32_t make_expected_group_count_sqlr(
    uint8_t *output, uint32_t key_type, uint8_t decimal_precision,
    uint8_t decimal_scale, const uint8_t *key_bytes, uint32_t key_len) {
    uint32_t offset = 0;
    memcpy(output + offset, "SQLR", 4);
    offset += 4;
    put_u16(output, &offset, 1);
    put_u16(output, &offset, 2);
    put_name(output, &offset, "id");
    put_u32(output, &offset, key_type);
    output[offset++] = decimal_precision;
    output[offset++] = decimal_scale;
    output[offset++] = PRDB_SDK_VALUE_STATE_VALUE;
    put_u32(output, &offset, key_len);
    memcpy(output + offset, key_bytes, key_len);
    offset += key_len;
    put_name(output, &offset, "count");
    put_u32(output, &offset, PRDB_SDK_VALUE_TYPE_BIGINT);
    output[offset++] = 0;
    output[offset++] = 0;
    output[offset++] = PRDB_SDK_VALUE_STATE_VALUE;
    put_u32(output, &offset, 8);
    memset(output + offset, 0, 7);
    output[offset + 7] = 1;
    offset += 8;
    return offset;
}

static int32_t on_group_v12(void *context,
                            const prdb_sdk_value_v1 *group_key,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v12 *binding) {
    group_capture *capture = (group_capture *)context;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V12_VERSION);
    assert(binding->order_count <= PRDB_SDK_SQL_V12_MAX_ORDER_KEYS);
    if (capture->scalar_order_mode) {
        uint32_t row = capture->calls;
        static const uint8_t id_name[] = "id";
        prdb_sdk_sql_column_shape_v2 group_column = {
            id_name, 2, 0, capture->scalar_order_type,
            capture->scalar_order_precision, capture->scalar_order_scale};
        uint8_t expected[128] = {0};
        uint32_t expected_len;
        assert(row < 3 && group_key != NULL && aggregate_value == NULL &&
               matching_row_count == 1);
        assert(binding->order_count == 1 &&
               binding->order_items[0].item.kind ==
                   PRDB_SDK_SQL_V10_ORDER_GROUP_KEY &&
               binding->order_items[0].item.direction ==
                   (capture->scalar_order_descending
                        ? PRDB_SDK_SQL_V10_ORDER_DESCENDING
                        : PRDB_SDK_SQL_V10_ORDER_ASCENDING) &&
               binding->order_items[0].type_code ==
                   capture->scalar_order_type);
        assert(group_key->state == PRDB_SDK_VALUE_STATE_VALUE &&
               group_key->type_code == capture->scalar_order_type &&
               group_key->data_len == capture->scalar_order_expected_len[row]);
        assert(memcmp(group_key->data, capture->scalar_order_expected[row],
                      group_key->data_len) == 0);
        expected_len = make_expected_group_count_sqlr(
            expected, capture->scalar_order_type,
            capture->scalar_order_precision, capture->scalar_order_scale,
            capture->scalar_order_expected[row],
            capture->scalar_order_expected_len[row]);
        assert(prdb_sdk_sql_encode_group_count_result_v1(
                   &group_column, group_key, matching_row_count,
                   capture->scalar_order_sqlr[row],
                   sizeof(capture->scalar_order_sqlr[row]),
                   &capture->scalar_order_sqlr_len[row]) ==
               PRDB_SDK_SQL_RESULT_OK);
        assert(capture->scalar_order_sqlr_len[row] == expected_len &&
               memcmp(capture->scalar_order_sqlr[row], expected,
                      expected_len) == 0);
        ++capture->calls;
        return 0;
    }
    if (binding->order_count == 2) {
        uint32_t expected_aggregate_type = PRDB_SDK_VALUE_TYPE_INTEGER;
        if (binding->base.base.base.base.aggregate_kind ==
                PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR ||
            binding->base.base.base.base.aggregate_kind ==
                PRDB_SDK_SQL_V7_AGGREGATE_SUM) {
            expected_aggregate_type = PRDB_SDK_VALUE_TYPE_BIGINT;
        } else if (binding->base.base.base.base.aggregate_kind ==
                   PRDB_SDK_SQL_V7_AGGREGATE_AVG) {
            expected_aggregate_type = PRDB_SDK_VALUE_TYPE_DECIMAL;
        }
        assert(binding->order_items[0].type_code == expected_aggregate_type);
        assert(binding->order_items[1].type_code ==
               (capture->mock->text_key_mode ? PRDB_SDK_VALUE_TYPE_TEXT
                                             : PRDB_SDK_VALUE_TYPE_INTEGER));
    }
    if (capture->mock->text_key_mode) {
        uint32_t key_index = 0;
        assert(group_key != NULL &&
               group_key->state == PRDB_SDK_VALUE_STATE_VALUE &&
               group_key->type_code == PRDB_SDK_VALUE_TYPE_TEXT &&
               group_key->data_len == capture->text_key_bytes);
        for (uint32_t index = group_key->data_len - 3;
             index < group_key->data_len; ++index) {
            assert(group_key->data[index] >= '0' &&
                   group_key->data[index] <= '9');
            key_index = key_index * 10 +
                        (uint32_t)(group_key->data[index] - '0');
        }
        assert(capture->calls < 128 && key_index < 128 &&
               matching_row_count == 1);
        capture->keys[capture->calls] = (int32_t)key_index;
        capture->aggregates[capture->calls] = 1;
        capture->row_counts[capture->calls] = matching_row_count;
        ++capture->calls;
        if (capture->mock->cancel_after_group_callbacks == capture->calls) {
            capture->mock->cancelled = 1;
        }
        return 0;
    }
    return on_group(context, group_key, aggregate_value, matching_row_count,
                    &binding->base.base);
}

static int32_t run_query_v12(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len, uint64_t arena_capacity,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[540000] = {0};
    uint8_t materialized_bytes[1048576] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    uint32_t key_type = capture->scalar_order_mode
                            ? capture->scalar_order_type
                            : mock->text_key_mode ? PRDB_SDK_VALUE_TYPE_TEXT
                                                  : PRDB_SDK_VALUE_TYPE_INTEGER;
    uint8_t key_precision = capture->scalar_order_mode
                                ? capture->scalar_order_precision
                                : 0;
    uint8_t key_scale = capture->scalar_order_mode
                            ? capture->scalar_order_scale
                            : 0;
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, key_type, key_precision, key_scale},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, key_type, key_precision, key_scale},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v12(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0,
        mock->max_candidates == 0 ? 4 : (uint16_t)mock->max_candidates,
        sizeof(batch_response),
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, arena_capacity, on_group_v12, capture);
}

typedef struct scalar_order_case {
    uint32_t type;
    uint8_t precision;
    uint8_t scale;
    uint32_t row_count;
    const uint8_t *lower;
    uint32_t lower_len;
    const uint8_t *middle;
    uint32_t middle_len;
    const uint8_t *higher;
    uint32_t higher_len;
} scalar_order_case;

static void reset_scan(mock_state *mock, group_capture *capture,
                       uint32_t finished);

static void run_grouped_scalar_order_matrix(mock_state *mock) {
    static const uint8_t asc_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id ORDER BY id ASC";
    static const uint8_t desc_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id ORDER BY id DESC";
    static const uint8_t boolean_lower[] = {0};
    static const uint8_t boolean_higher[] = {1};
    static const uint8_t smallint_lower[] = {0x80, 0};
    static const uint8_t smallint_middle[] = {0, 0};
    static const uint8_t smallint_higher[] = {0x7F, 0xFF};
    static const uint8_t integer_lower[] = {0x80, 0, 0, 0};
    static const uint8_t integer_middle[] = {0, 0, 0, 0};
    static const uint8_t integer_higher[] = {0x7F, 0xFF, 0xFF, 0xFF};
    static const uint8_t bigint_lower[] = {0x80, 0, 0, 0, 0, 0, 0, 0};
    static const uint8_t bigint_middle[] = {0, 0, 0, 0, 0, 0, 0, 0};
    static const uint8_t bigint_higher[] = {0x7F, 0xFF, 0xFF, 0xFF,
                                             0xFF, 0xFF, 0xFF, 0xFF};
    static const uint8_t decimal_lower[] = {0xFF, 0xFF, 0xFF, 0xFF,
                                             0xFF, 0xFF, 0xFF, 0xFF,
                                             0xFF, 0xFF, 0xFF, 0xFF,
                                             0xFF, 0xFF, 0xFF, 0x9C};
    static const uint8_t decimal_middle[16] = {0};
    static const uint8_t decimal_higher[] = {0, 0, 0, 0, 0, 0, 0, 0,
                                              0, 0, 0, 0, 0, 0, 0, 0x64};
    static const uint8_t text_lower[] = {'a'};
    static const uint8_t text_middle[] = {'b'};
    static const uint8_t text_higher[] = {'c'};
    static const uint8_t varchar_lower[] = {0xC2, 0xA2};
    static const uint8_t varchar_middle[] = {0xC3, 0xA9};
    static const uint8_t varchar_higher[] = {0xE2, 0x82, 0xAC};
    static const uint8_t bytea_lower[] = {0, 0};
    static const uint8_t bytea_middle[] = {0, 1};
    static const uint8_t bytea_higher[] = {1, 0};
    static const uint8_t uuid_lower[] = {0, 1, 2, 3, 4, 5, 6, 7,
                                          8, 9, 10, 11, 12, 13, 14, 15};
    static const uint8_t uuid_middle[] = {0, 1, 2, 3, 4, 5, 6, 7,
                                           8, 9, 10, 11, 12, 13, 14, 16};
    static const uint8_t uuid_higher[] = {0, 1, 2, 3, 4, 5, 6, 7,
                                           8, 9, 10, 11, 12, 13, 14, 17};
    static const uint8_t date_lower[] = {0xFF, 0xFF, 0xFF, 0xFF};
    static const uint8_t date_middle[] = {0, 0, 0, 0};
    static const uint8_t date_higher[] = {0, 0, 0, 1};
    static const uint8_t timestamp_lower[] = {0xFF, 0xFF, 0xFF, 0xFF,
                                               0xFF, 0xFF, 0xFF, 0xFF};
    static const uint8_t timestamp_middle[] = {0, 0, 0, 0, 0, 0, 0, 0};
    static const uint8_t timestamp_higher[] = {0, 0, 0, 0, 0, 0, 0, 1};
    static const scalar_order_case cases[] = {
        {PRDB_SDK_VALUE_TYPE_BOOLEAN, 0, 0, 2,
         boolean_lower, sizeof(boolean_lower), NULL, 0,
         boolean_higher, sizeof(boolean_higher)},
        {PRDB_SDK_VALUE_TYPE_SMALLINT, 0, 0, 3,
         smallint_lower, sizeof(smallint_lower), smallint_middle,
         sizeof(smallint_middle), smallint_higher, sizeof(smallint_higher)},
        {PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0, 3,
         integer_lower, sizeof(integer_lower), integer_middle,
         sizeof(integer_middle), integer_higher, sizeof(integer_higher)},
        {PRDB_SDK_VALUE_TYPE_BIGINT, 0, 0, 3,
         bigint_lower, sizeof(bigint_lower), bigint_middle,
         sizeof(bigint_middle), bigint_higher, sizeof(bigint_higher)},
        {PRDB_SDK_VALUE_TYPE_DECIMAL, 38, 2, 3,
         decimal_lower, sizeof(decimal_lower), decimal_middle,
         sizeof(decimal_middle), decimal_higher, sizeof(decimal_higher)},
        {PRDB_SDK_VALUE_TYPE_TEXT, 0, 0, 3,
         text_lower, sizeof(text_lower), text_middle, sizeof(text_middle),
         text_higher, sizeof(text_higher)},
        {PRDB_SDK_VALUE_TYPE_VARCHAR, 0, 0, 3,
         varchar_lower, sizeof(varchar_lower), varchar_middle,
         sizeof(varchar_middle), varchar_higher, sizeof(varchar_higher)},
        {PRDB_SDK_VALUE_TYPE_BYTEA, 0, 0, 3,
         bytea_lower, sizeof(bytea_lower), bytea_middle, sizeof(bytea_middle),
         bytea_higher, sizeof(bytea_higher)},
        {PRDB_SDK_VALUE_TYPE_UUID, 0, 0, 3,
         uuid_lower, sizeof(uuid_lower), uuid_middle, sizeof(uuid_middle),
         uuid_higher, sizeof(uuid_higher)},
        {PRDB_SDK_VALUE_TYPE_DATE, 0, 0, 3,
         date_lower, sizeof(date_lower), date_middle, sizeof(date_middle),
         date_higher, sizeof(date_higher)},
        {PRDB_SDK_VALUE_TYPE_TIMESTAMPTZ, 0, 0, 3,
         timestamp_lower, sizeof(timestamp_lower), timestamp_middle,
         sizeof(timestamp_middle), timestamp_higher, sizeof(timestamp_higher)},
    };

    for (uint32_t case_index = 0;
         case_index < sizeof(cases) / sizeof(cases[0]); ++case_index) {
        const scalar_order_case *test = &cases[case_index];
        const uint8_t *input_values[3] = {
            test->higher, test->lower, test->middle};
        uint32_t input_lengths[3] = {
            test->higher_len, test->lower_len, test->middle_len};
        const uint8_t *ascending_values[3] = {
            test->lower, test->middle, test->higher};
        uint32_t ascending_lengths[3] = {
            test->lower_len, test->middle_len, test->higher_len};
        const uint8_t *descending_values[3] = {
            test->higher, test->middle, test->lower};
        uint32_t descending_lengths[3] = {
            test->higher_len, test->middle_len, test->lower_len};

        mock->text_key_mode = 0;
        mock->max_candidates = 0;
        mock->planner_len = make_relation(
            mock->planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99,
            test->type);
        mock->executor_len = make_relation(mock->executor, 6, 0x22,
                                           test->type);
        mock->batch_len = make_typed_key_rows(
            mock->batch, 1, test->type, input_values, input_lengths,
            test->row_count);
        for (uint32_t direction = 0; direction < 2; ++direction) {
            group_capture capture = {0};
            capture.mock = mock;
            capture.scalar_order_mode = 1;
            capture.scalar_order_type = test->type;
            capture.scalar_order_precision = test->precision;
            capture.scalar_order_scale = test->scale;
            capture.scalar_order_descending = direction;
            for (uint32_t row = 0; row < test->row_count; ++row) {
                if (test->row_count == 2) {
                    capture.scalar_order_expected[row] = direction
                        ? (row == 0 ? test->higher : test->lower)
                        : (row == 0 ? test->lower : test->higher);
                    capture.scalar_order_expected_len[row] = direction
                        ? (row == 0 ? test->higher_len : test->lower_len)
                        : (row == 0 ? test->lower_len : test->higher_len);
                } else {
                    capture.scalar_order_expected[row] = direction
                        ? descending_values[row]
                        : ascending_values[row];
                    capture.scalar_order_expected_len[row] = direction
                        ? descending_lengths[row]
                        : ascending_lengths[row];
                }
            }
            reset_scan(mock, &capture, 1);
            int32_t status = run_query_v12(
                mock, direction ? desc_sql : asc_sql,
                direction ? sizeof(desc_sql) - 1 : sizeof(asc_sql) - 1, 1024,
                &capture);
            assert(status == PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == test->row_count &&
                   mock->statement_failed == 0);
            for (uint32_t row = 0; row < capture.calls; ++row) {
                assert(capture.scalar_order_sqlr_len[row] > 0);
            }
        }
    }
}

static int32_t on_group_v13(void *context,
                            const prdb_sdk_value_v1 *group_keys,
                            uint32_t group_key_count,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v13 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t first = 0, second = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V13_VERSION);
    assert(group_key_count == 2 && binding->group_key_count == 2);
    assert(capture->mock->batch_finished != 0);
    assert(capture->mock->executor_calls == 3);
    assert(capture->calls < 128);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &first) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[1], &second) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(binding->base.aggregate_kind ==
               PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR ||
           binding->base.aggregate_kind == PRDB_SDK_SQL_V7_AGGREGATE_SUM);
    if (binding->base.aggregate_kind ==
        PRDB_SDK_SQL_V7_AGGREGATE_COUNT_STAR) {
        assert(aggregate_value == NULL);
        capture->aggregates[capture->calls] = (int32_t)matching_row_count;
    } else {
        int64_t sum = 0;
        assert(aggregate_value != NULL &&
               aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_BIGINT);
        assert(prdb_sdk_value_get_bigint_v1(aggregate_value, &sum) ==
               PRDB_SDK_VALUE_ACCESS_OK);
        capture->aggregates[capture->calls] = (int32_t)sum;
        assert((int64_t)capture->aggregates[capture->calls] == sum);
    }
    capture->keys[capture->calls] = first;
    capture->second_keys[capture->calls] = second;
    capture->row_counts[capture->calls] = matching_row_count;
    ++capture->calls;
    if (capture->mock->cancel_after_group_callbacks == capture->calls) {
        capture->mock->cancelled = 1;
    }
    return 0;
}

static int32_t run_query_v13(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len, uint64_t arena_capacity,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[4096] = {0};
    uint8_t materialized_bytes[4096] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v13(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0,
        mock->max_candidates == 0 ? 4 : (uint16_t)mock->max_candidates,
        mock->max_candidates > 3 ? 4096 : 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, arena_capacity, on_group_v13, capture);
}

static int32_t on_group_v14(void *context,
                            const prdb_sdk_value_v1 *group_keys,
                            uint32_t group_key_count,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v14 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    int64_t sum = 0;
    (void)matching_row_count;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V14_VERSION);
    assert(binding->base.group_key_count == 2 && group_key_count == 2);
    assert(binding->has_having == 1 &&
           binding->having_parameter_index == 1);
    assert(capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_bigint_v1(aggregate_value, &sum) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = (int32_t)sum;
    ++capture->calls;
    return 0;
}

static int32_t run_query_v14(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len,
                             const prdb_sdk_value_v1 *parameter,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[64] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v14(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameter, 1, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v14, capture);
}

static int32_t on_group_v15(void *context,
                            const prdb_sdk_value_v1 *group_keys,
                            uint32_t group_key_count,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v15 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t first = 0, second = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V15_VERSION);
    assert(binding->order_count == 3);
    assert(binding->order_items[0].type_code == PRDB_SDK_VALUE_TYPE_INTEGER);
    assert(binding->order_items[1].type_code == PRDB_SDK_VALUE_TYPE_BIGINT);
    assert(binding->order_items[2].type_code == PRDB_SDK_VALUE_TYPE_INTEGER);
    assert(group_key_count == 2 && aggregate_value == NULL);
    assert(capture->mock->batch_finished != 0);
    assert(capture->mock->executor_calls == 3);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &first) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[1], &second) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    capture->keys[capture->calls] = first;
    capture->second_keys[capture->calls] = second;
    capture->row_counts[capture->calls] = matching_row_count;
    ++capture->calls;
    return 0;
}

static int32_t run_query_v15(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len,
                             const prdb_sdk_value_v1 *parameter,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[64] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v15(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameter, 1, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v15, capture);
}

static int32_t on_group_v17(void *context,
                            const prdb_sdk_value_v1 *group_keys,
                            uint32_t group_key_count,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v17 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    int64_t sum = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V17_VERSION);
    assert(binding->has_having_literal == capture->expect_having_literal);
    assert(binding->having_literal ==
           (capture->expect_having_literal ? UINT64_C(10) : UINT64_C(0)));
    assert(binding->base.base.has_having == 1);
    assert(binding->base.base.having_parameter_index == 1);
    assert(binding->base.base.base.group_key_count == 2 &&
           group_key_count == 2);
    assert(aggregate_value != NULL && matching_row_count == 2);
    assert(capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_bigint_v1(aggregate_value, &sum) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = (int32_t)sum;
    ++capture->calls;
    return 0;
}

static int32_t run_query_v17(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len,
                             const prdb_sdk_value_v1 *parameter,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[64] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v17(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameter,
        parameter == NULL ? 0 : 1, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v17, capture);
}

static int32_t on_group_v18(void *context,
                            const prdb_sdk_value_v1 *group_keys,
                            uint32_t group_key_count,
                            const prdb_sdk_value_v1 *aggregate_value,
                            uint64_t matching_row_count,
                            const prdb_sdk_select_binding_v18 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0, second_key = 0;
    int64_t sum = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V18_VERSION);
    assert(binding->having_node_count ==
           (capture->expect_having_literal ? 1u : 6u));
    assert(binding->having_root == binding->having_node_count - 1);
    assert(binding->having_type_code == PRDB_SDK_VALUE_TYPE_BIGINT);
    assert(binding->base.base.has_having == 0);
    assert(binding->base.base.base.group_key_count == 2 &&
           group_key_count == 2);
    assert(aggregate_value != NULL &&
           matching_row_count ==
               (capture->expected_row_count == 0
                    ? UINT64_C(1)
                    : capture->expected_row_count));
    assert(capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[1], &second_key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(prdb_sdk_value_get_bigint_v1(aggregate_value, &sum) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    capture->keys[capture->calls] = key;
    capture->second_keys[capture->calls] = second_key;
    capture->aggregates[capture->calls] = (int32_t)sum;
    ++capture->calls;
    return 0;
}

static int32_t run_query_v18(mock_state *mock, const uint8_t *sql,
                             uint64_t sql_len,
                             const prdb_sdk_value_v1 *parameter,
                             group_capture *capture) {
    static const uint8_t schema[] = "app";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[2048] = {0};
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v18(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameter,
        parameter == NULL ? 0 : 1,
        mock->max_candidates == 0 ? 4 : (uint16_t)mock->max_candidates, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v18, capture);
}

static int32_t on_group_v21_decimal(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    uint64_t aggregate_bits = 0;
    int64_t aggregate_coefficient;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_node_count == 1);
    assert(binding->having_nodes[0].base.operand_kind ==
           PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL);
    if (binding->base.having_decimal_scale == 2) {
        assert(binding->having_nodes[0].scale == 2);
        assert(binding->having_nodes[0].coefficient[14] == 1 &&
               binding->having_nodes[0].coefficient[15] == 0xc2);
    } else {
        assert(binding->base.having_decimal_scale == 8);
        assert(binding->having_nodes[0].scale == 8);
        assert(binding->having_nodes[0].coefficient[12] == 0x08 &&
               binding->having_nodes[0].coefficient[13] == 0x58 &&
               binding->having_nodes[0].coefficient[14] == 0x3b &&
               binding->having_nodes[0].coefficient[15] == 0x00);
    }
    assert(group_key_count == 2 && aggregate_value != NULL);
    assert(matching_row_count == 1 && capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           aggregate_value->data_len == 16);
    for (uint32_t index = 0; index < 8; ++index)
        assert(aggregate_value->data[index] == 0);
    for (uint32_t index = 0; index < 8; ++index)
        aggregate_bits = (aggregate_bits << 8) | aggregate_value->data[index + 8];
    aggregate_coefficient = (int64_t)aggregate_bits;
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = binding->base.having_decimal_scale == 2
                                              ? (int32_t)(aggregate_coefficient / 100)
                                              : (int32_t)(aggregate_coefficient /
                                                          1000000);
    ++capture->calls;
    if (capture->mock->cancel_after_group_callbacks == capture->calls)
        capture->mock->cancelled = 1;
    return 0;
}

static int32_t on_group_v22_null_sum(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_node_count == 3);
    assert(binding->having_nodes[0].base.node_kind ==
           PRDB_SDK_SQL_PREDICATE_COMPARE);
    assert(binding->having_nodes[0].base.operand_kind ==
        PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL);
    assert(binding->having_nodes[0].scale == 0 &&
           binding->having_nodes[0].coefficient[15] == 2);
    assert(binding->having_nodes[1].base.operand_kind ==
           PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL);
    assert(binding->having_nodes[1].scale == 2 &&
           binding->having_nodes[1].coefficient[14] == 1 &&
           binding->having_nodes[1].coefficient[15] == 0xc2);
    assert(group_key_count == 2 && matching_row_count == 2);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK && key == 1);
    assert(group_keys[1].state == PRDB_SDK_VALUE_STATE_NULL);
    assert(aggregate_value != NULL &&
           aggregate_value->state == PRDB_SDK_VALUE_STATE_NULL &&
           aggregate_value->type_code == 0);
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = -1;
    ++capture->calls;
    return 0;
}

static int32_t on_group_v22_hidden_count_avg(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_decimal_scale == 8);
    assert(group_key_count == 2 && matching_row_count == 2);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(key == 2 && group_keys[1].state == PRDB_SDK_VALUE_STATE_VALUE &&
           group_keys[1].type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           group_keys[1].data_len == 16 && group_keys[1].data[14] == 1 &&
           group_keys[1].data[15] == 44);
    assert(aggregate_value != NULL &&
           aggregate_value->state == PRDB_SDK_VALUE_STATE_VALUE &&
           aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           aggregate_value->data_len == 16);
    for (uint32_t index = 0; index < 12; ++index)
        assert(aggregate_value->data[index] == 0);
    assert(aggregate_value->data[12] == 0x11 &&
           aggregate_value->data[13] == 0xe1 &&
           aggregate_value->data[14] == 0xa3 &&
           aggregate_value->data[15] == 0x00);
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = 3;
    ++capture->calls;
    return 0;
}

static int32_t on_group_v22_hidden_count_minmax(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    int32_t key = 0;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_decimal_scale == 2);
    assert(group_key_count == 2 && matching_row_count == 2);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0], &key) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(key == 2 && group_keys[1].state == PRDB_SDK_VALUE_STATE_VALUE &&
           group_keys[1].type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           group_keys[1].data_len == 16 && group_keys[1].data[14] == 1 &&
           group_keys[1].data[15] == 44);
    assert(aggregate_value != NULL &&
           aggregate_value->state == PRDB_SDK_VALUE_STATE_VALUE &&
           aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           aggregate_value->data_len == 16);
    for (uint32_t index = 0; index < 14; ++index)
        assert(aggregate_value->data[index] == 0);
    assert(aggregate_value->data[14] == 1 &&
           aggregate_value->data[15] == 44);
    capture->keys[capture->calls] = key;
    capture->aggregates[capture->calls] = 3;
    ++capture->calls;
    return 0;
}

static int32_t on_group_v21_min_decimal(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_decimal_scale == 2);
    assert(binding->base.having_node_count == 1);
    assert(binding->having_nodes[0].scale == 2);
    assert(binding->having_nodes[0].base.operand_kind ==
           PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL);
    assert(binding->having_nodes[0].coefficient[14] == 1 &&
           binding->having_nodes[0].coefficient[15] == 44);
    assert(group_key_count == 2 && aggregate_value != NULL);
    assert(matching_row_count == 1 && capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0],
                                         &capture->keys[capture->calls]) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(group_keys[1].type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           group_keys[1].data_len == 16 && group_keys[1].data[14] == 1 &&
           group_keys[1].data[15] == 44);
    assert(aggregate_value->state == PRDB_SDK_VALUE_STATE_VALUE &&
           aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           aggregate_value->data_len == 16);
    for (uint32_t index = 0; index < 14; ++index)
        assert(aggregate_value->data[index] == 0);
    assert(aggregate_value->data[14] == 1 &&
           aggregate_value->data[15] == 44);
    capture->aggregates[capture->calls] = 300;
    ++capture->calls;
    return 0;
}

static int32_t on_group_v21_max_decimal(
    void *context, const prdb_sdk_value_v1 *group_keys,
    uint32_t group_key_count, const prdb_sdk_value_v1 *aggregate_value,
    uint64_t matching_row_count,
    const prdb_sdk_select_binding_v21 *binding) {
    group_capture *capture = (group_capture *)context;
    assert(binding->abi_version == PRDB_SDK_SQL_ABI_V21_VERSION);
    assert(binding->base.having_type_code == PRDB_SDK_VALUE_TYPE_DECIMAL);
    assert(binding->base.having_decimal_precision == 10);
    assert(binding->base.having_decimal_scale == 2);
    assert(binding->base.having_node_count == 1);
    assert(binding->having_nodes[0].scale == 2);
    assert(binding->having_nodes[0].base.operand_kind ==
           PRDB_SDK_SQL_V21_OPERAND_NUMERIC_LITERAL);
    assert(binding->having_nodes[0].coefficient[14] == 3 &&
           binding->having_nodes[0].coefficient[15] == 32);
    assert(group_key_count == 2 && aggregate_value != NULL);
    assert(matching_row_count == 1 && capture->mock->batch_finished != 0);
    assert(prdb_sdk_value_get_integer_v1(&group_keys[0],
                                         &capture->keys[capture->calls]) ==
           PRDB_SDK_VALUE_ACCESS_OK);
    assert(group_keys[1].type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           group_keys[1].data_len == 16 && group_keys[1].data[14] == 3 &&
           group_keys[1].data[15] == 132);
    assert(aggregate_value->state == PRDB_SDK_VALUE_STATE_VALUE &&
           aggregate_value->type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
           aggregate_value->data_len == 16);
    for (uint32_t index = 0; index < 14; ++index)
        assert(aggregate_value->data[index] == 0);
    assert(aggregate_value->data[14] == 3 &&
           aggregate_value->data[15] == 132);
    capture->aggregates[capture->calls] = 900;
    ++capture->calls;
    return 0;
}

static int32_t run_query_v21_decimal(mock_state *mock, const uint8_t *sql,
                                     uint64_t sql_len,
                                     group_capture *capture) {
    static const uint8_t schema[] = "app";
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[8192] = {0};
    uint8_t materialized_bytes[4096] = {0};
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v21(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0,
        mock->max_candidates == 0 ? 4 : (uint16_t)mock->max_candidates, 8192,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v21_decimal,
        capture);
}

static int32_t run_query_v22_decimal(mock_state *mock, const uint8_t *sql,
                                     uint64_t sql_len,
                                     const prdb_sdk_value_v1 *parameters,
                                     uint32_t parameter_count,
                                     prdb_sdk_sql_group_row_v22_fn on_group,
                                     group_capture *capture) {
    static const uint8_t schema[] = "app";
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[8192] = {0};
    uint8_t materialized_bytes[4096] = {0};
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v22(
        &host, schema, sizeof(schema) - 1, sql, sql_len, parameters,
        parameter_count,
        mock->max_candidates == 0 ? 4 : (uint16_t)mock->max_candidates, 8192,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group, capture);
}

static int32_t run_query_v21_decimal_wide_sum(
    mock_state *mock, const uint8_t *sql, uint64_t sql_len,
    group_capture *capture) {
    static const uint8_t schema[] = "app";
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[2048] = {0};
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 38, 2}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 38, 2}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v21(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes), on_group_v21_decimal,
        capture);
}

static int32_t run_query_v21_min_decimal(mock_state *mock, const uint8_t *sql,
                                         uint64_t sql_len,
                                         group_capture *capture) {
    static const uint8_t schema[] = "app";
    static const uint8_t id_name[] = "id";
    static const uint8_t score_name[] = "score";
    uint8_t planner_response[256] = {0};
    uint8_t executor_response[256] = {0};
    uint8_t batch_response[512] = {0};
    uint8_t materialized_bytes[2048] = {0};
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {id_name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 10, 2}};
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v21(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes),
        on_group_v21_min_decimal, capture);
}

static int32_t run_query_v21_max_decimal(mock_state *mock, const uint8_t *sql,
                                         uint64_t sql_len,
                                         group_capture *capture) {
    static const uint8_t schema[] = "app";
    static const uint8_t name[] = "id";
    static const uint8_t score_name[] = "score";
    prdb_sdk_sql_column_shape_v2 planner_columns[2] = {
        {name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 4, 2}};
    prdb_sdk_sql_column_shape_v2 executor_columns[2] = {
        {name, 2, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0},
        {score_name, 5, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 4, 2}};
    uint8_t planner_response[512];
    uint8_t executor_response[512];
    uint8_t batch_response[512];
    uint8_t materialized_bytes[4096];
    prdb_sdk_value_v1 cell_scratch[2] = {{0}};
    prdb_host_api_v3 host = {sizeof(host), PRDB_QUERY_ABI_V3_VERSION,
                             mock, executor_request, planner_request, NULL,
                             statement_failure};
    return prdb_sdk_sql_execute_group_v21(
        &host, schema, sizeof(schema) - 1, sql, sql_len, NULL, 0, 4, 512,
        planner_response, sizeof(planner_response), executor_response,
        sizeof(executor_response), batch_response, sizeof(batch_response),
        planner_columns, 2, executor_columns, 2, cell_scratch, 2,
        materialized_bytes, sizeof(materialized_bytes),
        on_group_v21_max_decimal, capture);
}

static void reset_scan(mock_state *mock, group_capture *capture,
                       uint32_t finished) {
    mock->executor_calls = 0;
    mock->executor_requests = 0;
    mock->next_calls = 0;
    mock->statement_failed = 0;
    mock->cancelled = 0;
    mock->cancel_after_first_batch = 0;
    mock->batch_finished = finished;
    capture->calls = 0;
}

int main(void) {
    static const uint8_t min_sql[] =
        "SELECT id, MIN(score) FROM widgets GROUP BY id";
    static const uint8_t max_sql[] =
        "SELECT id, MAX(score) FROM widgets GROUP BY id";
    static const uint8_t sum_sql[] =
        "SELECT id, SUM(score) FROM widgets GROUP BY id";
    static const uint8_t avg_sql[] =
        "SELECT id, AVG(score) FROM widgets GROUP BY id";
    static const uint8_t limit_sql[] =
        "SELECT id, MIN(score) FROM widgets GROUP BY id LIMIT 1";
    static const uint8_t where_sql[] =
        "SELECT id, MIN(score) FROM widgets WHERE score >= $1 GROUP BY id";
    static const uint8_t having_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "HAVING COUNT(*) = $1 LIMIT 1";
    static const uint8_t ordered_having_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "HAVING COUNT(*) >= $1 ORDER BY COUNT(*) ASC LIMIT 1";
    static const uint8_t ordered_key_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "ORDER BY id DESC LIMIT 1";
    static const uint8_t ordered_aggregate_sql[] =
        "SELECT id, MIN(score) FROM widgets GROUP BY id "
        "ORDER BY MIN(score) DESC LIMIT 1";
    static const uint8_t nulls_last_sql[] =
        "SELECT id, MIN(score) FROM widgets GROUP BY id "
        "ORDER BY MIN(score) ASC NULLS LAST";
    static const uint8_t default_null_order_sql[] =
        "SELECT id, MIN(score) FROM widgets GROUP BY id "
        "ORDER BY MIN(score) DESC";
    static const uint8_t composite_order_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "ORDER BY COUNT(*) ASC, id DESC NULLS LAST LIMIT 2";
    static const uint8_t ordered_group_limit_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "ORDER BY COUNT(*) DESC, id ASC LIMIT 128";
    static const uint8_t ordered_group_text_budget_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id "
        "ORDER BY COUNT(*) DESC, id ASC LIMIT 128";
    static const uint8_t *const ordered_aggregate_matrix_sql[] = {
        (const uint8_t *)"SELECT id, COUNT(*) FROM widgets GROUP BY id "
                         "ORDER BY COUNT(*) DESC NULLS LAST, id ASC NULLS FIRST",
        (const uint8_t *)"SELECT id, MIN(score) FROM widgets GROUP BY id "
                         "ORDER BY MIN(score) DESC NULLS LAST, id ASC NULLS FIRST",
        (const uint8_t *)"SELECT id, MAX(score) FROM widgets GROUP BY id "
                         "ORDER BY MAX(score) DESC NULLS LAST, id ASC NULLS FIRST",
        (const uint8_t *)"SELECT id, SUM(score) FROM widgets GROUP BY id "
                         "ORDER BY SUM(score) DESC NULLS LAST, id ASC NULLS FIRST",
        (const uint8_t *)"SELECT id, AVG(score) FROM widgets GROUP BY id "
                         "ORDER BY AVG(score) DESC NULLS LAST, id ASC NULLS FIRST",
    };
    static const uint32_t ordered_aggregate_matrix_sql_len[] = {
        sizeof("SELECT id, COUNT(*) FROM widgets GROUP BY id "
               "ORDER BY COUNT(*) DESC NULLS LAST, id ASC NULLS FIRST") - 1,
        sizeof("SELECT id, MIN(score) FROM widgets GROUP BY id "
               "ORDER BY MIN(score) DESC NULLS LAST, id ASC NULLS FIRST") - 1,
        sizeof("SELECT id, MAX(score) FROM widgets GROUP BY id "
               "ORDER BY MAX(score) DESC NULLS LAST, id ASC NULLS FIRST") - 1,
        sizeof("SELECT id, SUM(score) FROM widgets GROUP BY id "
               "ORDER BY SUM(score) DESC NULLS LAST, id ASC NULLS FIRST") - 1,
        sizeof("SELECT id, AVG(score) FROM widgets GROUP BY id "
               "ORDER BY AVG(score) DESC NULLS LAST, id ASC NULLS FIRST") - 1,
    };
    static const uint8_t v12_no_order_sql[] =
        "SELECT id, COUNT(*) FROM widgets GROUP BY id";
    static const uint8_t v13_composite_sql[] =
        "SELECT id, score, COUNT(*) FROM widgets GROUP BY id, score";
    static const uint8_t v13_sum_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score";
    static const uint8_t v14_having_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= $1";
    static const uint8_t v15_order_sql[] =
        "SELECT id, score, COUNT(*) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= $1 ORDER BY score DESC NULLS LAST, "
        "COUNT(*) DESC, id ASC NULLS LAST LIMIT 3";
    static const uint8_t v17_literal_having_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= 10 ORDER BY score DESC NULLS LAST, "
        "SUM(score) DESC, id ASC NULLS LAST LIMIT 3";
    static const uint8_t v17_parameter_having_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= $1 ORDER BY score DESC NULLS LAST, "
        "SUM(score) DESC, id ASC NULLS LAST LIMIT 3";
    static const uint8_t v17_overflow_having_sql[] =
        "SELECT id, score, COUNT(*) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) > 9223372036854775808";
    static const uint8_t v18_compound_having_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING (SUM(score) >= 9 OR SUM(score) = $1) AND "
        "NOT SUM(score) < 3 ORDER BY score DESC LIMIT 2";
    static const uint8_t v18_negative_literal_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= -1";
    static const uint8_t v21_decimal_literal_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= 4.50e0";
    static const uint8_t v22_hidden_count_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND SUM(score) >= 4.50e0";
    static const uint8_t v22_hidden_count_avg_sql[] =
        "SELECT id, score, AVG(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND AVG(score) >= 3.00000000e0";
    static const uint8_t v22_hidden_count_min_sql[] =
        "SELECT id, score, MIN(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND MIN(score) >= 3.00e0";
    static const uint8_t v22_hidden_count_max_sql[] =
        "SELECT id, score, MAX(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 AND MAX(score) <= 3.00e0";
    static const uint8_t v22_null_count_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= 2 OR SUM(score) >= 4.50e0";
    static const uint8_t v22_parameter_count_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING COUNT(*) >= $1 AND SUM(score) >= 4.50e0";
    static const uint8_t v21_decimal_equal_literal_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) = 4.50e0";
    static const uint8_t v21_decimal_avg_literal_sql[] =
        "SELECT id, score, AVG(score) FROM widgets GROUP BY id, score "
        "HAVING AVG(score) >= 1.40000000e0";
    static const uint8_t v21_decimal_min_literal_sql[] =
        "SELECT id, score, MIN(score) FROM widgets GROUP BY id, score "
        "HAVING MIN(score) <= 3.00e0 ORDER BY id ASC LIMIT 1";
    static const uint8_t v21_decimal_max_literal_sql[] =
        "SELECT id, score, MAX(score) FROM widgets GROUP BY id, score "
        "HAVING MAX(score) >= 8.00e0 ORDER BY id ASC LIMIT 1";
    static const uint8_t v21_decimal_sum_overflow_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= 0.00e0";
    static const uint8_t v21_decimal_inexact_scale_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= 1.251";
    static const uint8_t v21_decimal_overflow_literal_sql[] =
        "SELECT id, score, SUM(score) FROM widgets GROUP BY id, score "
        "HAVING SUM(score) >= 1e36";
    static const uint8_t v13_many_groups_sql[] =
        "SELECT id, score, COUNT(*) FROM widgets GROUP BY id, score";
    static const uint8_t parameter_bytes[] = {0, 0, 0, 5};
    static const uint8_t having_parameter_bytes[] = {
        0, 0, 0, 0, 0, 0, 0, 1};
    static const uint8_t v14_having_parameter_bytes[] = {
        0, 0, 0, 0, 0, 0, 0, 6};
    static const uint8_t v17_having_parameter_bytes[] = {
        0, 0, 0, 0, 0, 0, 0, 10};
    static const uint8_t v18_having_parameter_bytes[] = {
        0, 0, 0, 0, 0, 0, 0, 3};
    const int32_t keys[] = {1, 1, 2};
    const int32_t scores[] = {9, 3, 5};
    const uint8_t value_states[] = {
        PRDB_SDK_VALUE_STATE_VALUE,
        PRDB_SDK_VALUE_STATE_VALUE,
        PRDB_SDK_VALUE_STATE_VALUE};
    mock_state mock = {0};
    group_capture capture = {0};
    prdb_sdk_value_v1 parameter = {
        sizeof(parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
        parameter_bytes, sizeof(parameter_bytes)};
    prdb_sdk_value_v1 having_parameter = {
        sizeof(having_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        having_parameter_bytes, sizeof(having_parameter_bytes)};
    prdb_sdk_value_v1 v14_having_parameter = {
        sizeof(v14_having_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        v14_having_parameter_bytes, sizeof(v14_having_parameter_bytes)};
    prdb_sdk_value_v1 v15_having_parameter = {
        sizeof(v15_having_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        having_parameter_bytes, sizeof(having_parameter_bytes)};
    prdb_sdk_value_v1 v17_having_parameter = {
        sizeof(v17_having_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        v17_having_parameter_bytes, sizeof(v17_having_parameter_bytes)};
    prdb_sdk_value_v1 v18_having_parameter = {
        sizeof(v18_having_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
        PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
        v18_having_parameter_bytes, sizeof(v18_having_parameter_bytes)};
    mock.planner_len = make_relation(
        mock.planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99,
        PRDB_SDK_VALUE_TYPE_INTEGER);
    mock.executor_len = make_relation(mock.executor, 6, 0x22,
                                      PRDB_SDK_VALUE_TYPE_INTEGER);
    mock.batch_len = make_rows(mock.batch, 1, keys, scores, value_states, 3);
    mock.batch_finished = 1;
    capture.mock = &mock;

    assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.keys[0] == 1 &&
           capture.aggregates[0] == 3 && capture.row_counts[0] == 2 &&
           capture.keys[1] == 2 && capture.aggregates[1] == 5 &&
           capture.row_counts[1] == 1 && mock.statement_failed == 0);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, max_sql, sizeof(max_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.aggregates[0] == 9 &&
           capture.aggregates[1] == 5);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, sum_sql, sizeof(sum_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.keys[0] == 1 &&
           capture.aggregates[0] == 12 && capture.row_counts[0] == 2 &&
           capture.keys[1] == 2 && capture.aggregates[1] == 5 &&
           capture.row_counts[1] == 1);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, avg_sql, sizeof(avg_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.keys[0] == 1 &&
           capture.aggregates[0] == 6000000 && capture.row_counts[0] == 2 &&
           capture.keys[1] == 2 && capture.aggregates[1] == 5000000 &&
           capture.row_counts[1] == 1);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, limit_sql, sizeof(limit_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 1 && capture.keys[0] == 1 &&
           capture.aggregates[0] == 3);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, where_sql, sizeof(where_sql) - 1, &parameter, 1,
                     64, &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.keys[0] == 1 &&
           capture.aggregates[0] == 9 && capture.keys[1] == 2 &&
           capture.aggregates[1] == 5);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, having_sql, sizeof(having_sql) - 1,
                     &having_parameter, 1, 64, &capture) ==
           PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 1 && capture.keys[0] == 2 &&
           capture.aggregates[0] == 1);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, ordered_aggregate_sql,
                     sizeof(ordered_aggregate_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 1 && capture.keys[0] == 2 &&
           capture.aggregates[0] == 5);

    {
        const uint8_t null_then_value[] = {
            PRDB_SDK_VALUE_STATE_NULL,
            PRDB_SDK_VALUE_STATE_NULL,
            PRDB_SDK_VALUE_STATE_VALUE};
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   null_then_value, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v11(&mock, nulls_last_sql,
                             sizeof(nulls_last_sql) - 1, 64, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 2 &&
               capture.aggregates[0] == 5 && capture.keys[1] == 1 &&
               capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_NULL &&
               mock.statement_failed == 0);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v11(&mock, default_null_order_sql,
                             sizeof(default_null_order_sql) - 1, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 1 &&
               capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
               capture.keys[1] == 2 && capture.aggregates[1] == 5 &&
               mock.statement_failed == 0);
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    reset_scan(&mock, &capture, 1);
    assert(run_query_v12(&mock, v12_no_order_sql,
                         sizeof(v12_no_order_sql) - 1, 64, &capture) ==
           PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 2 && capture.keys[0] == 1 &&
           capture.keys[1] == 2 && capture.row_counts[0] == 2 &&
           capture.row_counts[1] == 1 && mock.statement_failed == 0);

    {
        const int32_t expected_keys[][2] = {
            {1, 2}, {2, 1}, {1, 2}, {1, 2}, {1, 2}};
        const int32_t expected_aggregates[][2] = {
            {2, 1}, {5, 3}, {9, 5}, {12, 5}, {6000000, 5000000}};
        for (uint32_t kind = 0; kind < 5; ++kind) {
            reset_scan(&mock, &capture, 1);
            assert(run_query_v12(
                       &mock, ordered_aggregate_matrix_sql[kind],
                       ordered_aggregate_matrix_sql_len[kind], 64,
                       &capture) == PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 2 &&
                   capture.keys[0] == expected_keys[kind][0] &&
                   capture.keys[1] == expected_keys[kind][1] &&
                   capture.aggregates[0] == expected_aggregates[kind][0] &&
                   capture.aggregates[1] == expected_aggregates[kind][1] &&
                   mock.statement_failed == 0);
        }
    }

    {
        int32_t many_group_keys[129];
        int32_t many_group_scores[129];
        uint8_t many_group_states[129];
        for (uint32_t index = 0; index < 129; ++index) {
            many_group_keys[index] = (int32_t)(129 - index);
            many_group_scores[index] = 1;
            many_group_states[index] = PRDB_SDK_VALUE_STATE_VALUE;
        }

        mock.batch_len = make_rows(mock.batch, 1, many_group_keys,
                                   many_group_scores, many_group_states, 128);
        mock.max_candidates = 128;
        reset_scan(&mock, &capture, 1);
        assert(run_query_v12(&mock, ordered_group_limit_sql,
                             sizeof(ordered_group_limit_sql) - 1, 8192,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 128 && mock.statement_failed == 0);
        for (uint32_t index = 0; index < 128; ++index) {
            assert(capture.keys[index] == (int32_t)(index + 2));
            assert(capture.aggregates[index] == 1);
        }

        mock.batch_len = make_rows(mock.batch, 1, many_group_keys,
                                   many_group_scores, many_group_states, 40);
        mock.max_candidates = 40;
        reset_scan(&mock, &capture, 1);
        mock.cancel_after_group_callbacks = 32;
        assert(run_query_v12(&mock, ordered_group_limit_sql,
                             sizeof(ordered_group_limit_sql) - 1, 8192,
                             &capture) ==
               PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
        assert(capture.calls == 32 && mock.cancelled == 1 &&
               mock.statement_failed == 1);
        mock.cancel_after_group_callbacks = 0;

        mock.batch_len = make_rows(mock.batch, 0, many_group_keys,
                                   many_group_scores, many_group_states, 128);
        mock.continued_batch_len = make_rows(
            mock.continued_batch, 1, many_group_keys + 128,
            many_group_scores + 128, many_group_states + 128, 1);
        mock.streamed_pages = 2;
        mock.max_candidates = 128;
        reset_scan(&mock, &capture, 1);
        int32_t over_limit_status = run_query_v12(
            &mock, ordered_group_limit_sql,
            sizeof(ordered_group_limit_sql) - 1, 8192, &capture);
        assert(over_limit_status == PRDB_SDK_SQL_EXEC_MATERIALIZATION_LIMIT);
        assert(capture.calls == 0 && mock.statement_failed == 1);
        mock.max_candidates = 0;
        mock.streamed_pages = 0;
        mock.continued_batch_len = 0;
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    {
        mock.text_key_mode = 1;
        mock.planner_len = make_relation(
            mock.planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99,
            PRDB_SDK_VALUE_TYPE_TEXT);
        mock.executor_len = make_relation(mock.executor, 6, 0x22,
                                          PRDB_SDK_VALUE_TYPE_TEXT);
        mock.batch_len = make_text_rows(mock.batch, 0, 0, 64, 8192);
        mock.continued_batch_len = make_text_rows(
            mock.continued_batch, 1, 64, 64, 8192);
        mock.streamed_pages = 2;
        mock.max_candidates = 64;
        capture.text_key_bytes = 8192;
        reset_scan(&mock, &capture, 1);
        assert(run_query_v12(&mock, ordered_group_text_budget_sql,
                             sizeof(ordered_group_text_budget_sql) - 1,
                             PRDB_SDK_SQL_EXEC_V7_MAX_MATERIALIZED_BYTES,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 128 && mock.statement_failed == 0);
        for (uint32_t index = 0; index < 128; ++index) {
            assert(capture.keys[index] == (int32_t)index);
        }

        mock.batch_len = make_text_rows(mock.batch, 0, 0, 64, 8193);
        mock.continued_batch_len = make_text_rows(
            mock.continued_batch, 1, 64, 64, 8193);
        capture.text_key_bytes = 8193;
        reset_scan(&mock, &capture, 1);
        assert(run_query_v12(&mock, ordered_group_text_budget_sql,
                             sizeof(ordered_group_text_budget_sql) - 1,
                             PRDB_SDK_SQL_EXEC_V7_MAX_MATERIALIZED_BYTES,
                             &capture) ==
               PRDB_SDK_SQL_EXEC_MATERIALIZATION_LIMIT);
        assert(capture.calls == 0 && mock.statement_failed == 1);

        mock.text_key_mode = 0;
        mock.max_candidates = 0;
        mock.streamed_pages = 0;
        mock.continued_batch_len = 0;
        capture.text_key_bytes = 0;
        mock.planner_len = make_relation(
            mock.planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99,
            PRDB_SDK_VALUE_TYPE_INTEGER);
        mock.executor_len = make_relation(mock.executor, 6, 0x22,
                                          PRDB_SDK_VALUE_TYPE_INTEGER);
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    {
        const int32_t tuple_ids[] = {1, 1, 2};
        const int32_t tuple_scores[] = {9, 9, 5};
        mock.batch_len = make_rows(mock.batch, 1, tuple_ids, tuple_scores,
                                   value_states, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v13(&mock, v13_composite_sql,
                             sizeof(v13_composite_sql) - 1, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 1 &&
               capture.second_keys[0] == 9 && capture.row_counts[0] == 2 &&
               capture.keys[1] == 2 && capture.second_keys[1] == 5 &&
               capture.row_counts[1] == 1 && mock.statement_failed == 0);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v13(&mock, v13_sum_sql,
                             sizeof(v13_sum_sql) - 1, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.aggregates[0] == 18 &&
               capture.aggregates[1] == 5 && mock.statement_failed == 0);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v14(&mock, v14_having_sql,
                             sizeof(v14_having_sql) - 1,
                             &v14_having_parameter, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 1 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 18 && mock.statement_failed == 0);

        reset_scan(&mock, &capture, 1);
        assert(run_query_v17(&mock, v17_overflow_having_sql,
                             sizeof(v17_overflow_having_sql) - 1, NULL,
                             &capture) == PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);

        {
            const int32_t nullable_keys[] = {1, 1, 2, 3};
            const int32_t nullable_scores[] = {9, 3, 5, 0};
            const uint8_t nullable_states[] = {
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_STATE_VALUE,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_STATE_NULL};
            mock.batch_len = make_rows(mock.batch, 1, nullable_keys,
                                       nullable_scores, nullable_states, 4);
            reset_scan(&mock, &capture, 1);
            /* The grouped inputs produce TRUE (9, 3), FALSE (5), and
             * UNKNOWN (the NULL-only group) for the compound HAVING tree. */
            assert(run_query_v18(&mock, v18_compound_having_sql,
                                 sizeof(v18_compound_having_sql) - 1,
                                 &v18_having_parameter, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 2 && capture.keys[0] == 1 &&
                   capture.second_keys[0] == 9 &&
                   capture.aggregates[0] == 9 && capture.keys[1] == 1 &&
                   capture.second_keys[1] == 3 &&
                   capture.aggregates[1] == 3 && mock.statement_failed == 0);

        reset_scan(&mock, &capture, 1);
        capture.expect_having_literal = 1;
        assert(run_query_v18(&mock, v18_negative_literal_sql,
                             sizeof(v18_negative_literal_sql) - 1, NULL,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 3 && capture.aggregates[0] == 9 &&
               capture.aggregates[1] == 3 && capture.aggregates[2] == 5 &&
               mock.statement_failed == 0);
        capture.expect_having_literal = 0;
        }
        {
            const int32_t one_key[] = {1};
            const int32_t one_score[] = {9};
            const uint8_t one_value_state[] = {
                PRDB_SDK_VALUE_STATE_VALUE};
            mock.batch_len = make_rows(mock.batch, 0, one_key, one_score,
                                       one_value_state, 1);
            mock.streamed_pages = 128;
            mock.max_candidates = 1;
            reset_scan(&mock, &capture, 1);
            capture.expected_row_count = 128;
            int32_t status = run_query_v18(
                &mock, v18_compound_having_sql,
                sizeof(v18_compound_having_sql) - 1, &v18_having_parameter,
                &capture);
            assert(capture.calls == 0);
            assert(mock.next_calls == 124);
            assert(mock.executor_requests == 251);
            assert(mock.statement_failed == 1);
            assert(status == PRDB_SDK_SQL_EXEC_REQUEST_LIMIT);
            mock.streamed_pages = 0;
            mock.max_candidates = 0;
            capture.expected_row_count = 0;
        }
        {
            int32_t many_keys[40];
            int32_t many_scores[40];
            uint8_t many_value_states[40];
            for (uint32_t index = 0; index < 40; ++index) {
                many_keys[index] = 1;
                many_scores[index] = (int32_t)index + 9;
                many_value_states[index] = PRDB_SDK_VALUE_STATE_VALUE;
            }
            mock.batch_len = make_rows(mock.batch, 1, many_keys, many_scores,
                                       many_value_states, 40);
            mock.max_candidates = 40;
            reset_scan(&mock, &capture, 1);
            mock.cancel_when_batch_finishes = 1;
            int32_t before_callback_status = run_query_v13(
                &mock, v13_many_groups_sql, sizeof(v13_many_groups_sql) - 1,
                4096, &capture);
            assert(before_callback_status == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
            assert(capture.calls == 0 && mock.cancelled == 1);
            assert(mock.executor_requests == 5 && mock.statement_failed == 1);

            reset_scan(&mock, &capture, 1);
            mock.cancel_when_batch_finishes = 0;
            mock.cancel_after_group_callbacks = 32;
            int32_t status = run_query_v13(
                &mock, v13_many_groups_sql, sizeof(v13_many_groups_sql) - 1,
                4096, &capture);
            assert(status == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
            assert(capture.calls == 32 && mock.cancelled == 1);
            assert(mock.executor_requests == 7 && mock.statement_failed == 1);
            mock.cancel_after_group_callbacks = 0;
            mock.max_candidates = 0;
        }
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    {
        const int32_t tuple_ids[] = {2, 1, 1, 0};
        const int32_t tuple_scores[] = {9, 9, 9, 9};
        const uint8_t tuple_states[] = {
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_STATE_VALUE,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_STATE_VALUE};
        const int32_t sorted_ids[] = {1, 0, 2};
        mock.batch_len = make_rows(mock.batch, 1, tuple_ids, tuple_scores,
                                   tuple_states, 4);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v15(&mock, v15_order_sql,
                             sizeof(v15_order_sql) - 1,
                             &v15_having_parameter, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 3 && mock.statement_failed == 0);
        for (uint32_t index = 0; index < 3; ++index) {
            assert(capture.keys[index] == sorted_ids[index]);
            assert(capture.second_keys[index] == 9);
        }
        assert(capture.row_counts[0] == 2 && capture.row_counts[1] == 1 &&
               capture.row_counts[2] == 1);

        reset_scan(&mock, &capture, 1);
        capture.expect_having_literal = 1;
        assert(run_query_v17(&mock, v17_literal_having_sql,
                             sizeof(v17_literal_having_sql) - 1,
                             NULL,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 1 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 18 && mock.statement_failed == 0);

        reset_scan(&mock, &capture, 1);
        capture.expect_having_literal = 0;
        assert(run_query_v17(&mock, v17_parameter_having_sql,
                             sizeof(v17_parameter_having_sql) - 1,
                             &v17_having_parameter, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 1 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 18 && mock.statement_failed == 0);

        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v18(&mock, v18_compound_having_sql,
                             sizeof(v18_compound_having_sql) - 1,
                             &v18_having_parameter, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 1 &&
               capture.second_keys[0] == 9 && capture.aggregates[0] == 9 &&
               capture.keys[1] == 1 && capture.second_keys[1] == 3 &&
               capture.aggregates[1] == 3 && mock.statement_failed == 0);

        reset_scan(&mock, &capture, 1);
        assert(run_query_v18(&mock, v18_compound_having_sql,
                             sizeof(v18_compound_having_sql) - 1, NULL,
                             &capture) == PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);

        reset_scan(&mock, &capture, 1);
        assert(run_query_v18(&mock, v18_compound_having_sql,
                             sizeof(v18_compound_having_sql) - 1, &parameter,
                             &capture) == PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    {
        const int32_t tied_group_keys[] = {1, 2, 3};
        mock.batch_len = make_rows(mock.batch, 1, tied_group_keys, scores,
                                   value_states, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v12(&mock, composite_order_sql,
                             sizeof(composite_order_sql) - 1, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 3 &&
               capture.keys[1] == 2 && capture.row_counts[0] == 1 &&
               capture.row_counts[1] == 1 && mock.statement_failed == 0);
        mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                   value_states, 3);
    }

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, ordered_having_sql,
                     sizeof(ordered_having_sql) - 1, &having_parameter, 1, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 1 && capture.keys[0] == 2 &&
           capture.aggregates[0] == 1);

    reset_scan(&mock, &capture, 1);
    assert(run_query(&mock, ordered_key_sql, sizeof(ordered_key_sql) - 1,
                     NULL, 0, 64, &capture) == PRDB_SDK_SQL_EXEC_OK);
    assert(capture.calls == 1 && capture.keys[0] == 2 &&
           capture.aggregates[0] == 1);

    {
        prdb_sdk_value_v1 null_parameter = {
            sizeof(null_parameter), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_NULL, 0, NULL, 0};
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, having_sql, sizeof(having_sql) - 1,
                         &null_parameter, 1, 64, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 0 && mock.next_calls == 1 &&
               mock.statement_failed == 0);
    }

    {
        prdb_sdk_value_v1 invalid_parameter = parameter;
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, having_sql, sizeof(having_sql) - 1,
                         &invalid_parameter, 1, 64, &capture) ==
               PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);
        invalid_parameter.state = PRDB_SDK_VALUE_STATE_REDACTED;
        invalid_parameter.type_code = 0;
        invalid_parameter.data = NULL;
        invalid_parameter.data_len = 0;
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, having_sql, sizeof(having_sql) - 1,
                         &invalid_parameter, 1, 64, &capture) ==
               PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);
    }

    {
        const uint8_t null_states[] = {
            PRDB_SDK_VALUE_STATE_NULL,
            PRDB_SDK_VALUE_STATE_NULL,
            PRDB_SDK_VALUE_STATE_NULL};
        mock.batch_len = make_rows(mock.batch, 1, keys, scores, null_states, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 64,
                         &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 &&
               capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
               capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_NULL);
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, max_sql, sizeof(max_sql) - 1, NULL, 0, 64,
                         &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 &&
               capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
               capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_NULL);
        {
            const uint8_t partly_null_states[] = {
                PRDB_SDK_VALUE_STATE_NULL,
                PRDB_SDK_VALUE_STATE_NULL,
                PRDB_SDK_VALUE_STATE_VALUE};
            mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                       partly_null_states, 3);
            reset_scan(&mock, &capture, 1);
            assert(run_query(&mock, ordered_aggregate_sql,
                             sizeof(ordered_aggregate_sql) - 1, NULL, 0, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 1 &&
                   capture.aggregate_states[0] ==
                       PRDB_SDK_VALUE_STATE_NULL);
            reset_scan(&mock, &capture, 1);
            assert(run_query(&mock, max_sql, sizeof(max_sql) - 1, NULL, 0, 64,
                             &capture) == PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 2 &&
                   capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
                   capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_VALUE &&
                   capture.aggregates[1] == 5);
            mock.batch_len = make_rows(mock.batch, 1, keys, scores,
                                       null_states, 3);
        }
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, avg_sql, sizeof(avg_sql) - 1, NULL, 0, 64,
                         &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 &&
               capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
               capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_NULL);
        reset_scan(&mock, &capture, 1);
        assert(run_query(&mock, sum_sql, sizeof(sum_sql) - 1, NULL, 0, 64,
                         &capture) == PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 &&
               capture.aggregate_states[0] == PRDB_SDK_VALUE_STATE_NULL &&
               capture.aggregate_states[1] == PRDB_SDK_VALUE_STATE_NULL);
    }

    {
        const uint8_t redacted_states[] = {
            PRDB_SDK_VALUE_STATE_VALUE,
            PRDB_SDK_VALUE_STATE_REDACTED,
            PRDB_SDK_VALUE_STATE_VALUE};
        mock.batch_len = make_rows(mock.batch, 0, keys, scores,
                                   redacted_states, 3);
        reset_scan(&mock, &capture, 0);
        assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 64,
                         &capture) ==
               PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
        assert(capture.calls == 0 && mock.executor_calls == 4 &&
               mock.statement_failed == 1);
        reset_scan(&mock, &capture, 0);
        assert(run_query(&mock, max_sql, sizeof(max_sql) - 1, NULL, 0, 64,
                         &capture) ==
               PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
        assert(capture.calls == 0 && mock.executor_calls == 4 &&
               mock.statement_failed == 1);
        reset_scan(&mock, &capture, 0);
        assert(run_query(&mock, avg_sql, sizeof(avg_sql) - 1, NULL, 0, 64,
                         &capture) ==
               PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
        assert(capture.calls == 0 && mock.executor_calls == 4 &&
               mock.statement_failed == 1);
        reset_scan(&mock, &capture, 0);
        assert(run_query(&mock, sum_sql, sizeof(sum_sql) - 1, NULL, 0, 64,
                         &capture) ==
               PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
        assert(capture.calls == 0 && mock.executor_calls == 4 &&
               mock.statement_failed == 1);
    }

    mock.batch_len = make_rows(mock.batch, 0, keys, scores, value_states, 3);
    reset_scan(&mock, &capture, 0);
    assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 4,
                     &capture) ==
           PRDB_SDK_SQL_EXEC_MATERIALIZATION_LIMIT);
    assert(capture.calls == 0 && mock.executor_calls == 4 &&
           mock.statement_failed == 1);

    reset_scan(&mock, &capture, 0);
    mock.cancelled = 1;
    assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
    assert(capture.calls == 0 && mock.next_calls == 0 &&
           mock.executor_calls == 3 && mock.statement_failed == 1);

    mock.batch_len = make_rows(mock.batch, 0, keys, scores, value_states, 3);
    reset_scan(&mock, &capture, 0);
    mock.cancel_after_first_batch = 1;
    assert(run_query(&mock, min_sql, sizeof(min_sql) - 1, NULL, 0, 64,
                     &capture) == PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
    assert(capture.calls == 0 && mock.next_calls == 1 &&
           mock.executor_calls == 4 && mock.statement_failed == 1);

    {
        const int64_t decimal_coefficients[] = {900, 300, 500};
        mock.planner_len = make_decimal_relation(
            mock.planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99, 10, 2);
        mock.executor_len = make_decimal_relation(mock.executor, 6, 0x22,
                                                  10, 2);
        mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                           decimal_coefficients, NULL, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v22_decimal(
                   &mock, v22_hidden_count_sql,
                   sizeof(v22_hidden_count_sql) - 1, NULL, 0,
                   on_group_v21_decimal, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 0 && mock.statement_failed == 0);

        {
            const int32_t avg_keys[] = {2, 2};
            const int64_t avg_coefficients[] = {300, 300};
            mock.batch_len = make_decimal_rows(mock.batch, 1, avg_keys,
                                               avg_coefficients, NULL, 2);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_hidden_count_avg_sql,
                       sizeof(v22_hidden_count_avg_sql) - 1, NULL, 0,
                       on_group_v22_hidden_count_avg, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 2 &&
                   capture.aggregates[0] == 3 && mock.statement_failed == 0);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_hidden_count_min_sql,
                       sizeof(v22_hidden_count_min_sql) - 1, NULL, 0,
                       on_group_v22_hidden_count_minmax, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 2 &&
                   capture.aggregates[0] == 3 && mock.statement_failed == 0);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_hidden_count_max_sql,
                       sizeof(v22_hidden_count_max_sql) - 1, NULL, 0,
                       on_group_v22_hidden_count_minmax, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 2 &&
                   capture.aggregates[0] == 3 && mock.statement_failed == 0);
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               decimal_coefficients, NULL, 3);
        }

        {
            const int32_t null_keys[] = {1, 1};
            const int64_t null_coefficients[] = {0, 0};
            const uint8_t null_states[] = {PRDB_SDK_VALUE_STATE_NULL,
                                           PRDB_SDK_VALUE_STATE_NULL};
            mock.batch_len = make_decimal_rows(mock.batch, 1, null_keys,
                                               null_coefficients, null_states,
                                               2);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_null_count_sql,
                       sizeof(v22_null_count_sql) - 1, NULL, 0,
                       on_group_v22_null_sum, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 1 &&
                   capture.aggregates[0] == -1 && mock.statement_failed == 0);
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               decimal_coefficients, NULL, 3);
        }

        {
            const uint8_t threshold_bytes[8] = {0, 0, 0, 0, 0, 0, 0, 2};
            const uint8_t decimal_bytes[16] = {0};
            prdb_sdk_value_v1 count_parameter = {
                sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
                threshold_bytes, sizeof(threshold_bytes)};
            prdb_sdk_value_v1 wrong_parameter = {
                sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
                decimal_bytes, sizeof(decimal_bytes)};
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_parameter_count_sql,
                       sizeof(v22_parameter_count_sql) - 1, &count_parameter,
                       1, on_group_v21_decimal, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 0 && mock.statement_failed == 0);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v22_decimal(
                       &mock, v22_parameter_count_sql,
                       sizeof(v22_parameter_count_sql) - 1, &wrong_parameter,
                       1, on_group_v21_decimal, &capture) ==
                   PRDB_SDK_SQL_EXEC_BIND_ERROR);
            assert(capture.calls == 0 && mock.next_calls == 0 &&
                   mock.statement_failed == 1);
        }

        reset_scan(&mock, &capture, 1);
        assert(run_query_v21_decimal(
                   &mock, v21_decimal_literal_sql,
                   sizeof(v21_decimal_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 2 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 9 && capture.keys[1] == 2 &&
               capture.aggregates[1] == 5 && mock.statement_failed == 0);

        {
            const int64_t exact_coefficients[] = {450, 449, 500};
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               exact_coefficients, NULL, 3);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v21_decimal(
                       &mock, v21_decimal_equal_literal_sql,
                       sizeof(v21_decimal_equal_literal_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 1 && capture.keys[0] == 1 &&
                   capture.aggregates[0] == 4 && mock.statement_failed == 0);
        }

        {
            const int64_t average_coefficients[] = {150, 130, 250};
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               average_coefficients, NULL, 3);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v21_decimal(
                       &mock, v21_decimal_avg_literal_sql,
                       sizeof(v21_decimal_avg_literal_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 2 && capture.keys[0] == 1 &&
                   capture.aggregates[0] == 150 && capture.keys[1] == 2 &&
                   capture.aggregates[1] == 250 && mock.statement_failed == 0);
        }

        reset_scan(&mock, &capture, 1);
        assert(run_query_v21_decimal(
                   &mock, v21_decimal_inexact_scale_sql,
                   sizeof(v21_decimal_inexact_scale_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);

        reset_scan(&mock, &capture, 1);
        assert(run_query_v21_decimal(
                   &mock, v21_decimal_overflow_literal_sql,
                   sizeof(v21_decimal_overflow_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_BIND_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);

        reset_scan(&mock, &capture, 1);
        mock.cancelled = 1;
        assert(run_query_v21_decimal(
                   &mock, v21_decimal_literal_sql,
                   sizeof(v21_decimal_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 0 &&
               mock.statement_failed == 1);

        reset_scan(&mock, &capture, 1);
        mock.cancel_after_first_batch = 1;
        assert(run_query_v21_decimal(
                   &mock, v21_decimal_literal_sql,
                   sizeof(v21_decimal_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
        assert(capture.calls == 0 && mock.next_calls == 1 &&
               mock.statement_failed == 1);

        {
            int32_t many_decimal_keys[40];
            int64_t many_decimal_coefficients[40];
            for (uint32_t index = 0; index < 40; ++index) {
                many_decimal_keys[index] = (int32_t)(index + 1);
                many_decimal_coefficients[index] = 900;
            }
            mock.batch_len = make_decimal_rows(
                mock.batch, 1, many_decimal_keys,
                many_decimal_coefficients, NULL, 40);
            mock.max_candidates = 40;
            reset_scan(&mock, &capture, 1);
            mock.cancel_after_group_callbacks = 32;
            assert(run_query_v21_decimal(
                       &mock, v21_decimal_literal_sql,
                       sizeof(v21_decimal_literal_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_EXECUTOR_ERROR);
            assert(capture.calls == 32 && mock.cancelled == 1 &&
                   mock.statement_failed == 1);
            mock.cancel_after_group_callbacks = 0;
            mock.max_candidates = 0;
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               decimal_coefficients, NULL, 3);
        }

        mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                           decimal_coefficients, NULL, 3);
        reset_scan(&mock, &capture, 1);
        assert(run_query_v21_min_decimal(
                   &mock, v21_decimal_min_literal_sql,
                   sizeof(v21_decimal_min_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 1 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 300 && mock.statement_failed == 0);

        reset_scan(&mock, &capture, 1);
        assert(run_query_v21_max_decimal(
                   &mock, v21_decimal_max_literal_sql,
                   sizeof(v21_decimal_max_literal_sql) - 1, &capture) ==
               PRDB_SDK_SQL_EXEC_OK);
        assert(capture.calls == 1 && capture.keys[0] == 1 &&
               capture.aggregates[0] == 900 && mock.statement_failed == 0);

        {
            const uint8_t decimal_null_states[] = {
                PRDB_SDK_VALUE_STATE_NULL, PRDB_SDK_VALUE_STATE_NULL,
                PRDB_SDK_VALUE_STATE_NULL};
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               decimal_coefficients,
                                               decimal_null_states, 3);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v21_max_decimal(
                       &mock, v21_decimal_max_literal_sql,
                       sizeof(v21_decimal_max_literal_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_OK);
            assert(capture.calls == 0 && mock.statement_failed == 0);
        }

        {
            const uint8_t decimal_redacted_states[] = {
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_STATE_REDACTED,
                PRDB_SDK_VALUE_STATE_VALUE};
            mock.batch_len = make_decimal_rows(mock.batch, 1, keys,
                                               decimal_coefficients,
                                               decimal_redacted_states, 3);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v21_max_decimal(
                       &mock, v21_decimal_max_literal_sql,
                       sizeof(v21_decimal_max_literal_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
            assert(capture.calls == 0 && mock.executor_calls > 0 &&
                   mock.statement_failed == 1);
        }

        {
            const int32_t overflow_keys[] = {1, 1};
            static const uint8_t max_decimal_coefficient[2][16] = {
                {0x4B, 0x3B, 0x4C, 0xA8, 0x5A, 0x86, 0xC4, 0x7A,
                 0x09, 0x8A, 0x22, 0x3F, 0xFF, 0xFF, 0xFF, 0xFF},
                {0x4B, 0x3B, 0x4C, 0xA8, 0x5A, 0x86, 0xC4, 0x7A,
                 0x09, 0x8A, 0x22, 0x3F, 0xFF, 0xFF, 0xFF, 0xFF}};
            mock.planner_len = make_decimal_relation(
                mock.planner, PRDB_SDK_PLANNER_TABLE_HANDLE_KIND, 0x99, 38,
                2);
            mock.executor_len = make_decimal_relation(mock.executor, 6, 0x22,
                                                      38, 2);
            mock.batch_len = make_decimal_rows_bytes(
                mock.batch, 1, overflow_keys, max_decimal_coefficient, 2);
            reset_scan(&mock, &capture, 1);
            assert(run_query_v21_decimal_wide_sum(
                       &mock, v21_decimal_sum_overflow_sql,
                       sizeof(v21_decimal_sum_overflow_sql) - 1, &capture) ==
                   PRDB_SDK_SQL_EXEC_EVALUATION_ERROR);
            assert(capture.calls == 0 && mock.executor_calls > 0 &&
                   mock.statement_failed == 1);
        }
    }
    run_grouped_scalar_order_matrix(&mock);
    return 0;
}
