#include "prdb_sdk_sql_group_v1.h"

#include <assert.h>
#include <string.h>

static prdb_sdk_value_v1 integer_value(uint8_t bytes[4], int32_t value) {
    uint32_t bits = (uint32_t)value;
    bytes[0] = (uint8_t)(bits >> 24);
    bytes[1] = (uint8_t)(bits >> 16);
    bytes[2] = (uint8_t)(bits >> 8);
    bytes[3] = (uint8_t)bits;
    return (prdb_sdk_value_v1){sizeof(prdb_sdk_value_v1),
                               PRDB_SDK_VALUE_ABI_V1_VERSION,
                               PRDB_SDK_VALUE_STATE_VALUE,
                               PRDB_SDK_VALUE_TYPE_INTEGER,
                               bytes,
                               4};
}

static prdb_sdk_value_v1 null_value(void) {
    return (prdb_sdk_value_v1){sizeof(prdb_sdk_value_v1),
                               PRDB_SDK_VALUE_ABI_V1_VERSION,
                               PRDB_SDK_VALUE_STATE_NULL,
                               0,
                               NULL,
                               0};
}

static prdb_sdk_value_v1 text_value(const char *value) {
    return (prdb_sdk_value_v1){sizeof(prdb_sdk_value_v1),
                               PRDB_SDK_VALUE_ABI_V1_VERSION,
                               PRDB_SDK_VALUE_STATE_VALUE,
                               PRDB_SDK_VALUE_TYPE_TEXT,
                               (const uint8_t *)value,
                               strlen(value)};
}

static prdb_sdk_value_v1 bigint_value(uint8_t bytes[8], int64_t value) {
    uint64_t bits = (uint64_t)value;
    for (uint32_t index = 0; index < 8; ++index) {
        bytes[7 - index] = (uint8_t)(bits >> (index * 8));
    }
    return (prdb_sdk_value_v1){sizeof(prdb_sdk_value_v1),
                               PRDB_SDK_VALUE_ABI_V1_VERSION,
                               PRDB_SDK_VALUE_STATE_VALUE,
                               PRDB_SDK_VALUE_TYPE_BIGINT,
                               bytes,
                               8};
}

static prdb_sdk_value_v1 scalar_value(uint32_t type, const uint8_t *bytes,
                                     uint32_t length) {
    return (prdb_sdk_value_v1){sizeof(prdb_sdk_value_v1),
                               PRDB_SDK_VALUE_ABI_V1_VERSION,
                               PRDB_SDK_VALUE_STATE_VALUE, type, bytes, length};
}

static void assert_scalar_aggregate_type(uint32_t type, uint8_t decimal_scale,
                                         const uint8_t *lower,
                                         uint32_t lower_len,
                                         const uint8_t *higher,
                                         uint32_t higher_len) {
    prdb_sdk_value_v1 values[3] = {
        scalar_value(type, higher, higher_len),
        null_value(),
        scalar_value(type, lower, lower_len)};
    prdb_sdk_value_v1 result = {0};
    uint8_t group_bytes[3][16] = {{0}};
    prdb_sdk_value_v1 group_keys[3] = {
        scalar_value(type, higher, higher_len),
        scalar_value(type, lower, lower_len),
        scalar_value(type, higher, higher_len)};
    prdb_sdk_sql_count_group_v1 groups[3] = {{0}};
    uint32_t group_count = 0;

    assert(prdb_sdk_sql_minmax_v1(values, 3, type, decimal_scale,
                                  PRDB_SDK_SQL_MINMAX_MIN, &result,
                                  sizeof(result)) == PRDB_SDK_SQL_EVAL_OK);
    assert(result.type_code == type && result.data_len == lower_len &&
           memcmp(result.data, lower, lower_len) == 0);
    assert(prdb_sdk_sql_minmax_v1(values, 3, type, decimal_scale,
                                  PRDB_SDK_SQL_MINMAX_MAX, &result,
                                  sizeof(result)) == PRDB_SDK_SQL_EVAL_OK);
    assert(result.type_code == type && result.data_len == higher_len &&
           memcmp(result.data, higher, higher_len) == 0);

    /* Include distinct storage for each state so each row is an independent
     * value view, including the repeated value used for the equality group. */
    memcpy(group_bytes[0], higher, higher_len);
    memcpy(group_bytes[1], lower, lower_len);
    memcpy(group_bytes[2], higher, higher_len);
    group_keys[0] = scalar_value(type, group_bytes[0], higher_len);
    group_keys[1] = scalar_value(type, group_bytes[1], lower_len);
    group_keys[2] = scalar_value(type, group_bytes[2], higher_len);
    assert(prdb_sdk_sql_group_count_v1(
               group_keys, 3, type, decimal_scale, groups, 3,
               &group_count) == PRDB_SDK_SQL_GROUP_OK);
    assert(group_count == 2 && groups[0].representative_row == 0 &&
           groups[0].count == 2 && groups[1].representative_row == 1 &&
           groups[1].count == 1);
}

static void test_analytical_scalar_type_matrix(void) {
    static const uint8_t bool_false[] = {0};
    static const uint8_t bool_true[] = {1};
    static const uint8_t smallint_lower[] = {0xFF, 0xFF};
    static const uint8_t smallint_higher[] = {0, 1};
    static const uint8_t integer_lower[] = {0xFF, 0xFF, 0xFF, 0xFF};
    static const uint8_t integer_higher[] = {0, 0, 0, 1};
    static const uint8_t bigint_lower[] = {0xFF, 0xFF, 0xFF, 0xFF,
                                           0xFF, 0xFF, 0xFF, 0xFF};
    static const uint8_t bigint_higher[] = {0, 0, 0, 0, 0, 0, 0, 1};
    static const uint8_t decimal_lower[] = {0, 0, 0, 0, 0, 0, 0, 0,
                                            0, 0, 0, 0, 0, 0, 0, 100};
    static const uint8_t decimal_higher[] = {0, 0, 0, 0, 0, 0, 0, 0,
                                             0, 0, 0, 0, 0, 0, 0, 200};
    static const uint8_t text_lower[] = {'a'};
    static const uint8_t text_higher[] = {'b'};
    static const uint8_t varchar_lower[] = {0xC3, 0xA9};
    static const uint8_t varchar_higher[] = {0xE2, 0x82, 0xAC};
    static const uint8_t bytea_lower[] = {0, 0xFF};
    static const uint8_t bytea_higher[] = {1, 0};
    static const uint8_t uuid_lower[] = {0, 1, 2, 3, 4, 5, 6, 7,
                                         8, 9, 10, 11, 12, 13, 14, 15};
    static const uint8_t uuid_higher[] = {0, 1, 2, 3, 4, 5, 6, 7,
                                          8, 9, 10, 11, 12, 13, 14, 16};
    static const uint8_t date_lower[] = {0, 0, 0, 0};
    static const uint8_t date_higher[] = {0, 0, 0, 1};
    static const uint8_t timestamp_lower[] = {0, 0, 0, 0, 0, 0, 0, 0};
    static const uint8_t timestamp_higher[] = {0, 0, 0, 0, 0, 0, 0, 1};

    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_BOOLEAN, 0,
                                 bool_false, sizeof(bool_false), bool_true,
                                 sizeof(bool_true));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_SMALLINT, 0,
                                 smallint_lower, sizeof(smallint_lower),
                                 smallint_higher, sizeof(smallint_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                                 integer_lower, sizeof(integer_lower),
                                 integer_higher, sizeof(integer_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_BIGINT, 0,
                                 bigint_lower, sizeof(bigint_lower),
                                 bigint_higher, sizeof(bigint_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_DECIMAL, 2,
                                 decimal_lower, sizeof(decimal_lower),
                                 decimal_higher, sizeof(decimal_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_TEXT, 0, text_lower,
                                 sizeof(text_lower), text_higher,
                                 sizeof(text_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_VARCHAR, 0,
                                 varchar_lower, sizeof(varchar_lower),
                                 varchar_higher, sizeof(varchar_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_BYTEA, 0, bytea_lower,
                                 sizeof(bytea_lower), bytea_higher,
                                 sizeof(bytea_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_UUID, 0, uuid_lower,
                                 sizeof(uuid_lower), uuid_higher,
                                 sizeof(uuid_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_DATE, 0, date_lower,
                                 sizeof(date_lower), date_higher,
                                 sizeof(date_higher));
    assert_scalar_aggregate_type(PRDB_SDK_VALUE_TYPE_TIMESTAMPTZ, 0,
                                 timestamp_lower, sizeof(timestamp_lower),
                                 timestamp_higher, sizeof(timestamp_higher));
}

int main(void) {
    uint8_t bytes[3][4];
    prdb_sdk_value_v1 keys[] = {
        integer_value(bytes[0], 8), integer_value(bytes[1], 3),
        integer_value(bytes[2], 8), null_value(), null_value()};
    prdb_sdk_sql_count_group_v1 groups[5] = {{0}};
    uint32_t group_count = 999;

    test_analytical_scalar_type_matrix();

    assert(prdb_sdk_sql_group_count_v1(
               keys, 5, PRDB_SDK_VALUE_TYPE_INTEGER, 0, groups, 5,
               &group_count) == PRDB_SDK_SQL_GROUP_OK);
    assert(group_count == 3);
    assert(groups[0].representative_row == 0 && groups[0].count == 2);
    assert(groups[1].representative_row == 1 && groups[1].count == 1);
    assert(groups[2].representative_row == 3 && groups[2].count == 2);

    {
        uint8_t composite_integer_bytes[4][4];
        prdb_sdk_value_v1 composite_keys[] = {
            integer_value(composite_integer_bytes[0], 1), text_value("x"),
            integer_value(composite_integer_bytes[1], 1), text_value("y"),
            integer_value(composite_integer_bytes[2], 1), text_value("x"),
            null_value(), text_value("x"),
            null_value(), text_value("y"),
            null_value(), text_value("x")};
        const uint32_t declared_types[] = {
            PRDB_SDK_VALUE_TYPE_INTEGER, PRDB_SDK_VALUE_TYPE_TEXT};
        const uint8_t decimal_scales[] = {0, 0};
        prdb_sdk_sql_count_group_v1 composite_groups[6] = {{0}};
        uint32_t composite_group_count = 99;
        assert(prdb_sdk_sql_group_count_composite_v1(
                   composite_keys, 6, 2, declared_types, decimal_scales,
                   composite_groups, 6, &composite_group_count) ==
               PRDB_SDK_SQL_GROUP_OK);
        assert(composite_group_count == 4);
        assert(composite_groups[0].representative_row == 0 &&
               composite_groups[0].count == 2);
        assert(composite_groups[1].representative_row == 1 &&
               composite_groups[1].count == 1);
        assert(composite_groups[2].representative_row == 3 &&
               composite_groups[2].count == 2);
        assert(composite_groups[3].representative_row == 4 &&
               composite_groups[3].count == 1);

        {
            prdb_sdk_sql_count_group_v1 unchanged = {81, UINT64_C(82)};
            uint32_t unchanged_count = 83;
            assert(prdb_sdk_sql_group_count_composite_v1(
                       composite_keys, PRDB_SDK_SQL_GROUP_MAX_ROWS + 1, 2,
                       declared_types, decimal_scales, &unchanged, 1,
                       &unchanged_count) ==
                   PRDB_SDK_SQL_GROUP_LIMIT_EXCEEDED);
            assert(unchanged_count == 83 &&
                   unchanged.representative_row == 81 && unchanged.count == 82);
            assert(prdb_sdk_sql_group_count_composite_v1(
                       composite_keys, 6,
                       PRDB_SDK_SQL_GROUP_MAX_COLUMNS + 1,
                       declared_types, decimal_scales, &unchanged, 1,
                       &unchanged_count) ==
                   PRDB_SDK_SQL_EVAL_INVALID_ARGUMENT);
            assert(unchanged_count == 83 && unchanged.representative_row == 81);
            assert(prdb_sdk_sql_group_count_composite_v1(
                       NULL, 0, 2, declared_types, decimal_scales, NULL, 0,
                       &unchanged_count) == PRDB_SDK_SQL_GROUP_OK);
            assert(unchanged_count == 0);
        }

        {
            prdb_sdk_sql_count_group_v1 unchanged = {71, UINT64_C(72)};
            uint32_t unchanged_count = 73;
            prdb_sdk_value_v1 invalid_keys[12];
            memcpy(invalid_keys, composite_keys, sizeof(invalid_keys));
            invalid_keys[5] = (prdb_sdk_value_v1){
                sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_REDACTED, 0, NULL, 0};
            assert(prdb_sdk_sql_group_count_composite_v1(
                       invalid_keys, 6, 2, declared_types, decimal_scales,
                       &unchanged, 1, &unchanged_count) ==
                   PRDB_SDK_SQL_EVAL_REDACTED_INPUT);
            assert(unchanged_count == 73 &&
                   unchanged.representative_row == 71 && unchanged.count == 72);

            assert(prdb_sdk_sql_group_count_composite_v1(
                       composite_keys, 6, 2, declared_types, decimal_scales,
                       &unchanged, 1, &unchanged_count) ==
                   PRDB_SDK_SQL_EVAL_INVALID_ARGUMENT);
            assert(unchanged_count == 73 &&
                   unchanged.representative_row == 71 && unchanged.count == 72);
        }
    }

    {
        prdb_sdk_sql_count_group_v1 unchanged[2] = {
            {41, UINT64_C(42)}, {43, UINT64_C(44)}};
        uint32_t unchanged_count = 45;
        uint8_t bigint_bytes[8] = {0};
        prdb_sdk_value_v1 invalid_keys[2] = {keys[0], keys[1]};
        invalid_keys[1] = (prdb_sdk_value_v1){
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_REDACTED, 0, NULL, 0};
        assert(prdb_sdk_sql_group_count_v1(
                   invalid_keys, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   unchanged, 2, &unchanged_count) ==
               PRDB_SDK_SQL_EVAL_REDACTED_INPUT);
        assert(unchanged_count == 45 && unchanged[0].representative_row == 41 &&
               unchanged[0].count == 42 &&
               unchanged[1].representative_row == 43 &&
               unchanged[1].count == 44);

        invalid_keys[1] = (prdb_sdk_value_v1){
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
            bigint_bytes, sizeof(bigint_bytes)};
        assert(prdb_sdk_sql_group_count_v1(
                   invalid_keys, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   unchanged, 2, &unchanged_count) ==
               PRDB_SDK_SQL_EVAL_TYPE_MISMATCH);
        assert(unchanged_count == 45 && unchanged[0].representative_row == 41);

        invalid_keys[1] = (prdb_sdk_value_v1){
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
            bytes[1], 3};
        assert(prdb_sdk_sql_group_count_v1(
                   invalid_keys, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   unchanged, 2, &unchanged_count) ==
               PRDB_SDK_SQL_EVAL_INVALID_VALUE);
        assert(unchanged_count == 45 && unchanged[0].representative_row == 41);

        assert(prdb_sdk_sql_group_count_v1(
                   keys, 5, PRDB_SDK_VALUE_TYPE_INTEGER, 0, unchanged, 2,
                   &unchanged_count) == PRDB_SDK_SQL_EVAL_INVALID_ARGUMENT);
        assert(unchanged_count == 45 && unchanged[0].representative_row == 41);
    }

    {
        prdb_sdk_value_v1 too_many[PRDB_SDK_SQL_GROUP_MAX_ROWS + 1] = {0};
        prdb_sdk_sql_count_group_v1 unchanged = {77, UINT64_C(78)};
        uint32_t unchanged_count = 79;
        assert(prdb_sdk_sql_group_count_v1(
                   too_many, PRDB_SDK_SQL_GROUP_MAX_ROWS + 1,
                   PRDB_SDK_VALUE_TYPE_INTEGER, 0, &unchanged, 1,
                   &unchanged_count) == PRDB_SDK_SQL_GROUP_LIMIT_EXCEEDED);
        assert(unchanged_count == 79 && unchanged.representative_row == 77 &&
               unchanged.count == 78);
    }

    {
        uint8_t key_bytes[PRDB_SDK_SQL_GROUP_MAX_ROWS][4];
        prdb_sdk_value_v1 boundary_keys[PRDB_SDK_SQL_GROUP_MAX_ROWS];
        prdb_sdk_sql_count_group_v1 boundary_groups[PRDB_SDK_SQL_GROUP_MAX_ROWS] =
            {{0}};
        uint32_t boundary_group_count = UINT32_MAX;
        for (uint32_t index = 0; index < PRDB_SDK_SQL_GROUP_MAX_ROWS; ++index) {
            boundary_keys[index] = integer_value(key_bytes[index], 7);
        }
        assert(prdb_sdk_sql_group_count_v1(
                   boundary_keys, PRDB_SDK_SQL_GROUP_MAX_ROWS,
                   PRDB_SDK_VALUE_TYPE_INTEGER, 0, boundary_groups,
                   PRDB_SDK_SQL_GROUP_MAX_ROWS, &boundary_group_count) ==
               PRDB_SDK_SQL_GROUP_OK);
        assert(boundary_group_count == 1);
        assert(boundary_groups[0].representative_row == 0 &&
               boundary_groups[0].count == PRDB_SDK_SQL_GROUP_MAX_ROWS);

        for (uint32_t index = 0; index < PRDB_SDK_SQL_GROUP_MAX_ROWS; ++index) {
            boundary_keys[index] = integer_value(key_bytes[index], (int32_t)index);
        }
        boundary_group_count = UINT32_MAX;
        assert(prdb_sdk_sql_group_count_v1(
                   boundary_keys, PRDB_SDK_SQL_GROUP_MAX_ROWS,
                   PRDB_SDK_VALUE_TYPE_INTEGER, 0, boundary_groups,
                   PRDB_SDK_SQL_GROUP_MAX_ROWS, &boundary_group_count) ==
               PRDB_SDK_SQL_GROUP_OK);
        assert(boundary_group_count == PRDB_SDK_SQL_GROUP_MAX_ROWS);
        for (uint32_t index = 0; index < boundary_group_count; ++index) {
            assert(boundary_groups[index].representative_row == index);
            assert(boundary_groups[index].count == 1);
        }
    }

    group_count = 999;
    assert(prdb_sdk_sql_group_count_v1(
               NULL, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, NULL, 0,
               &group_count) == PRDB_SDK_SQL_GROUP_OK);
    assert(group_count == 0);

    {
        prdb_sdk_value_v1 result = {
            sizeof(result), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
            bytes[0], sizeof(bytes[0])};
        uint8_t bigint_bytes[8] = {0};
        int32_t result_number = 0;
        assert(prdb_sdk_sql_minmax_v1(
                   keys, 5, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MIN, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_OK);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
               PRDB_SDK_VALUE_ACCESS_OK);
        assert(result_number == 3);
        assert(prdb_sdk_sql_minmax_v1(
                   keys, 5, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MAX, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_OK);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
               PRDB_SDK_VALUE_ACCESS_OK);
        assert(result_number == 8);
        assert(prdb_sdk_sql_minmax_v1(
                   NULL, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MIN, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_OK);
        assert(result.state == PRDB_SDK_VALUE_STATE_NULL &&
               result.type_code == 0 && result.data == NULL &&
               result.data_len == 0);
        assert(prdb_sdk_sql_minmax_v1(
                   &keys[3], 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MAX, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_OK);
        assert(result.state == PRDB_SDK_VALUE_STATE_NULL);

        result = (prdb_sdk_value_v1){
            sizeof(result), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
            bytes[0], sizeof(bytes[0])};
        prdb_sdk_value_v1 invalid_values[2] = {0};
        invalid_values[0] = keys[0];
        invalid_values[1] = (prdb_sdk_value_v1){
            sizeof(result), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_REDACTED, 0, NULL, 0};
        assert(prdb_sdk_sql_minmax_v1(
                   invalid_values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MIN, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_REDACTED_INPUT);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
                   PRDB_SDK_VALUE_ACCESS_OK &&
               result_number == 8);
        invalid_values[1] = (prdb_sdk_value_v1){
            sizeof(result), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_BIGINT,
            bigint_bytes, sizeof(bigint_bytes)};
        assert(prdb_sdk_sql_minmax_v1(
                   invalid_values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MAX, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_TYPE_MISMATCH);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
                   PRDB_SDK_VALUE_ACCESS_OK &&
               result_number == 8);
        invalid_values[1] = (prdb_sdk_value_v1){
            sizeof(result), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_INTEGER,
            bytes[1], 3};
        assert(prdb_sdk_sql_minmax_v1(
                   invalid_values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MIN, &result, sizeof(result)) ==
               PRDB_SDK_SQL_EVAL_INVALID_VALUE);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
                   PRDB_SDK_VALUE_ACCESS_OK &&
               result_number == 8);
        assert(prdb_sdk_sql_minmax_v1(
                   keys, 5, PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                   PRDB_SDK_SQL_MINMAX_MIN, &result, 1) ==
               PRDB_SDK_SQL_EVAL_INVALID_ARGUMENT);
        assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
                   PRDB_SDK_VALUE_ACCESS_OK &&
               result_number == 8);
        {
            prdb_sdk_value_v1 too_many[PRDB_SDK_SQL_GROUP_MAX_ROWS + 1] = {0};
            assert(prdb_sdk_sql_minmax_v1(
                       too_many, PRDB_SDK_SQL_GROUP_MAX_ROWS + 1,
                       PRDB_SDK_VALUE_TYPE_INTEGER, 0,
                       PRDB_SDK_SQL_MINMAX_MAX, &result, sizeof(result)) ==
                   PRDB_SDK_SQL_GROUP_LIMIT_EXCEEDED);
            assert(prdb_sdk_value_get_integer_v1(&result, &result_number) ==
                       PRDB_SDK_VALUE_ACCESS_OK &&
                   result_number == 8);
        }
    }

    {
        uint8_t decimal_bytes[2][16] = {{0}};
        prdb_sdk_value_v1 decimal_values[2] = {{0}};
        prdb_sdk_value_v1 decimal_min = {0};
        decimal_bytes[0][15] = 0xB6;
        decimal_bytes[1][15] = 0x78;
        decimal_bytes[1][14] = 0x03;
        decimal_values[0] = (prdb_sdk_value_v1){
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
            decimal_bytes[0], sizeof(decimal_bytes[0])};
        decimal_values[1] = (prdb_sdk_value_v1){
            sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
            PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
            decimal_bytes[1], sizeof(decimal_bytes[1])};
        assert(prdb_sdk_sql_minmax_v1(
                   decimal_values, 2, PRDB_SDK_VALUE_TYPE_DECIMAL, 2,
                   PRDB_SDK_SQL_MINMAX_MIN, &decimal_min,
                   sizeof(decimal_min)) == PRDB_SDK_SQL_EVAL_OK);
        assert(decimal_min.data == decimal_bytes[0]);
    }
    {
        uint8_t integer_bytes[3][8] = {{0}};
        uint8_t result_bytes[16];
        prdb_sdk_value_v1 sum_values[3] = {
            bigint_value(integer_bytes[0], 10),
            bigint_value(integer_bytes[1], -3), null_value()};
        prdb_sdk_value_v1 sum = {0};
        int64_t sum_number = 0;

        memset(result_bytes, 0xA5, sizeof(result_bytes));
        assert(prdb_sdk_sql_sum_v1(
                   sum_values, 3, PRDB_SDK_VALUE_TYPE_BIGINT, 0, 0, &sum,
                   sizeof(sum), result_bytes, sizeof(result_bytes)) ==
               PRDB_SDK_SQL_EVAL_OK);
        assert(sum.type_code == PRDB_SDK_VALUE_TYPE_BIGINT &&
               sum.data == result_bytes && sum.data_len == 8);
        assert(prdb_sdk_value_get_bigint_v1(&sum, &sum_number) ==
               PRDB_SDK_VALUE_ACCESS_OK);
        assert(sum_number == 7);

        {
            uint8_t cancellation_bytes[3][8] = {{0}};
            prdb_sdk_value_v1 cancellation_values[3] = {
                bigint_value(cancellation_bytes[0], INT64_MAX),
                bigint_value(cancellation_bytes[1], 1),
                bigint_value(cancellation_bytes[2], -1)};
            assert(prdb_sdk_sql_sum_v1(
                       cancellation_values, 3, PRDB_SDK_VALUE_TYPE_BIGINT, 0,
                       0, &sum, sizeof(sum), result_bytes,
                       sizeof(result_bytes)) == PRDB_SDK_SQL_EVAL_OK);
            assert(prdb_sdk_value_get_bigint_v1(&sum, &sum_number) ==
                   PRDB_SDK_VALUE_ACCESS_OK);
            assert(sum_number == INT64_MAX);
        }

        {
            uint8_t overflow_bytes[2][8] = {{0}};
            prdb_sdk_value_v1 overflow_values[2] = {
                bigint_value(overflow_bytes[0], INT64_MAX),
                bigint_value(overflow_bytes[1], 1)};
            prdb_sdk_value_v1 unchanged_sum = sum;
            uint8_t before[16];
            memcpy(before, result_bytes, sizeof(before));
            assert(prdb_sdk_sql_sum_v1(
                       overflow_values, 2, PRDB_SDK_VALUE_TYPE_BIGINT, 0, 0,
                       &unchanged_sum, sizeof(unchanged_sum), result_bytes,
                       sizeof(result_bytes)) ==
                   PRDB_SDK_SQL_EVAL_NUMERIC_OVERFLOW);
            assert(unchanged_sum.data == sum.data &&
                   unchanged_sum.data_len == sum.data_len &&
                   memcmp(result_bytes, before, sizeof(before)) == 0);
        }

        {
            uint8_t decimal_bytes[2][16] = {{0}};
            prdb_sdk_value_v1 decimal_values[2] = {{0}};
            prdb_sdk_value_v1 decimal_sum = {0};
            decimal_bytes[0][15] = 123;
            memset(decimal_bytes[1], 0xFF, sizeof(decimal_bytes[1]));
            decimal_bytes[1][15] = 0xE9;
            decimal_values[0] = (prdb_sdk_value_v1){
                sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
                decimal_bytes[0], sizeof(decimal_bytes[0])};
            decimal_values[1] = (prdb_sdk_value_v1){
                sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
                decimal_bytes[1], sizeof(decimal_bytes[1])};
            assert(prdb_sdk_sql_sum_v1(
                       decimal_values, 2, PRDB_SDK_VALUE_TYPE_DECIMAL, 5, 2,
                       &decimal_sum, sizeof(decimal_sum), result_bytes,
                       sizeof(result_bytes)) == PRDB_SDK_SQL_EVAL_OK);
            assert(decimal_sum.type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
                   decimal_sum.data_len == 16 &&
                   decimal_sum.data == result_bytes &&
                   result_bytes[15] == 100);
        }

        {
            uint8_t decimal_bytes[3][16] = {{0}};
            prdb_sdk_value_v1 decimal_values[3] = {{0}};
            prdb_sdk_value_v1 decimal_sum = {0};
            static const uint8_t max_decimal_coefficient[16] = {
                0x4B, 0x3B, 0x4C, 0xA8, 0x5A, 0x86, 0xC4, 0x7A,
                0x09, 0x8A, 0x22, 0x3F, 0xFF, 0xFF, 0xFF, 0xFF};
            for (uint32_t index = 0; index < 3; ++index) {
                memcpy(decimal_bytes[index], max_decimal_coefficient,
                       sizeof(max_decimal_coefficient));
                decimal_values[index] = (prdb_sdk_value_v1){
                    sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                    PRDB_SDK_VALUE_STATE_VALUE,
                    PRDB_SDK_VALUE_TYPE_DECIMAL, decimal_bytes[index], 16};
            }
            assert(prdb_sdk_sql_sum_v1(
                       decimal_values, 3, PRDB_SDK_VALUE_TYPE_DECIMAL, 38, 0,
                       &decimal_sum, sizeof(decimal_sum), result_bytes,
                       sizeof(result_bytes)) ==
                   PRDB_SDK_SQL_EVAL_NUMERIC_OVERFLOW);
        }

        assert(prdb_sdk_sql_sum_v1(
                   NULL, 0, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0, &sum,
                   sizeof(sum), NULL, 0) == PRDB_SDK_SQL_EVAL_OK);
        assert(sum.state == PRDB_SDK_VALUE_STATE_NULL && sum.type_code == 0 &&
               sum.data == NULL && sum.data_len == 0);
    }
    {
        uint8_t input_bytes[2][4] = {{0}};
        uint8_t result_bytes[16];
        prdb_sdk_value_v1 values[2] = {integer_value(input_bytes[0], 1),
                                       integer_value(input_bytes[1], 2)};
        prdb_sdk_value_v1 average = {0};
        uint8_t result_scale = 99;
        static const uint8_t expected_half[] = {
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x16, 0xE3, 0x60};
        assert(prdb_sdk_sql_avg_v1(
                   values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0, &average,
                   sizeof(average), result_bytes, sizeof(result_bytes),
                   &result_scale) == PRDB_SDK_SQL_EVAL_OK);
        assert(average.type_code == PRDB_SDK_VALUE_TYPE_DECIMAL &&
               average.data == result_bytes && average.data_len == 16 &&
               result_scale == 6 &&
               memcmp(result_bytes, expected_half, sizeof(expected_half)) == 0);

        {
            uint8_t negative_bytes[2][4] = {{0}};
            prdb_sdk_value_v1 negative_values[2] = {
                integer_value(negative_bytes[0], -1),
                integer_value(negative_bytes[1], -2)};
            static const uint8_t expected_negative[] = {
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE9, 0x1C, 0xA0};
            assert(prdb_sdk_sql_avg_v1(
                       negative_values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(result_scale == 6 &&
                   memcmp(result_bytes, expected_negative,
                          sizeof(expected_negative)) == 0);
        }

        {
            uint8_t many_bytes[128][4] = {{0}};
            prdb_sdk_value_v1 many_values[128] = {{0}};
            static const uint8_t expected_even[] = {
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1E, 0x84};
            static const uint8_t expected_odd[] = {
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5B, 0x8E};
            static const uint8_t expected_negative_even[] = {
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xE1, 0x7C};
            static const uint8_t expected_negative_odd[] = {
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xA4, 0x72};
            for (uint32_t index = 0; index < 128; ++index) {
                many_values[index] = integer_value(many_bytes[index], 0);
            }
            many_values[0] = integer_value(many_bytes[0], 1);
            assert(prdb_sdk_sql_avg_v1(
                       many_values, 128, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(memcmp(result_bytes, expected_even, sizeof(expected_even)) ==
                   0);
            many_values[0] = integer_value(many_bytes[0], 3);
            assert(prdb_sdk_sql_avg_v1(
                       many_values, 128, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(memcmp(result_bytes, expected_odd, sizeof(expected_odd)) ==
                   0);
            many_values[0] = integer_value(many_bytes[0], -1);
            assert(prdb_sdk_sql_avg_v1(
                       many_values, 128, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(memcmp(result_bytes, expected_negative_even,
                          sizeof(expected_negative_even)) == 0);
            many_values[0] = integer_value(many_bytes[0], -3);
            assert(prdb_sdk_sql_avg_v1(
                       many_values, 128, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(memcmp(result_bytes, expected_negative_odd,
                          sizeof(expected_negative_odd)) == 0);
        }

        {
            uint8_t decimal_bytes[2][16] = {{0}};
            prdb_sdk_value_v1 decimal_values[2] = {{0}};
            static const uint8_t expected_decimal[] = {
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x07, 0x5C, 0x75, 0xE0};
            decimal_bytes[0][15] = 123;
            decimal_bytes[1][15] = 124;
            for (uint32_t index = 0; index < 2; ++index) {
                decimal_values[index] = (prdb_sdk_value_v1){
                    sizeof(prdb_sdk_value_v1), PRDB_SDK_VALUE_ABI_V1_VERSION,
                    PRDB_SDK_VALUE_STATE_VALUE,
                    PRDB_SDK_VALUE_TYPE_DECIMAL, decimal_bytes[index], 16};
            }
            assert(prdb_sdk_sql_avg_v1(
                       decimal_values, 2, PRDB_SDK_VALUE_TYPE_DECIMAL, 5, 2,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(result_scale == 8 &&
                   memcmp(result_bytes, expected_decimal,
                          sizeof(expected_decimal)) == 0);
        }

        {
            static const uint8_t max_decimal[16] = {
                0x4B, 0x3B, 0x4C, 0xA8, 0x5A, 0x86, 0xC4, 0x7A,
                0x09, 0x8A, 0x22, 0x3F, 0xFF, 0xFF, 0xFF, 0xFF};
            prdb_sdk_value_v1 max_value = {
                sizeof(max_value), PRDB_SDK_VALUE_ABI_V1_VERSION,
                PRDB_SDK_VALUE_STATE_VALUE, PRDB_SDK_VALUE_TYPE_DECIMAL,
                max_decimal, sizeof(max_decimal)};
            assert(prdb_sdk_sql_avg_v1(
                       &max_value, 1, PRDB_SDK_VALUE_TYPE_DECIMAL, 38, 0,
                       &average, sizeof(average), result_bytes,
                       sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(result_scale == 0 &&
                   memcmp(result_bytes, max_decimal, sizeof(max_decimal)) ==
                       0);
        }

        {
            prdb_sdk_value_v1 nulls[2] = {null_value(), null_value()};
            assert(prdb_sdk_sql_avg_v1(
                       nulls, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0, &average,
                       sizeof(average), NULL, 0, &result_scale) ==
                   PRDB_SDK_SQL_EVAL_OK);
            assert(average.state == PRDB_SDK_VALUE_STATE_NULL &&
                   average.type_code == 0 && average.data == NULL &&
                   average.data_len == 0 && result_scale == 6);
        }

        {
            prdb_sdk_value_v1 redacted = null_value();
            prdb_sdk_value_v1 invalid_values[2] = {values[0], redacted};
            prdb_sdk_value_v1 unchanged_average = average;
            uint8_t before[16];
            memset(result_bytes, 0xA5, sizeof(result_bytes));
            memcpy(before, result_bytes, sizeof(before));
            result_scale = 99;
            redacted.state = PRDB_SDK_VALUE_STATE_REDACTED;
            invalid_values[1] = redacted;
            unchanged_average.struct_size = UINT32_C(0x12345678);
            assert(prdb_sdk_sql_avg_v1(
                       invalid_values, 2, PRDB_SDK_VALUE_TYPE_INTEGER, 0, 0,
                       &unchanged_average, sizeof(unchanged_average),
                       result_bytes, sizeof(result_bytes), &result_scale) ==
                   PRDB_SDK_SQL_EVAL_REDACTED_INPUT);
            assert(unchanged_average.struct_size == UINT32_C(0x12345678) &&
                   result_scale == 99 &&
                   memcmp(result_bytes, before, sizeof(before)) == 0);
        }
    }
    return 0;
}
