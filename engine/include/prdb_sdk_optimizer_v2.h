#ifndef PRDB_SDK_OPTIMIZER_V2_H
#define PRDB_SDK_OPTIMIZER_V2_H

#include "prdb_query_abi_v5.h"
#include "prdb_sdk_optimizer_v1.h"
#include "prdb_sdk_sql_dml_v1.h"

/* Encode a parsed SQL DELETE with its mandatory bounded WHERE tree as
 * optimizer request v1. Predicate parameters use the same supported scalar
 * subset as the parameterized SELECT encoder. */
int32_t prdb_sdk_optimizer_encode_delete_v1(
    const prdb_sdk_delete_v1 *statement,
    const prdb_sdk_value_v1 *parameters, uint32_t parameter_count,
    const uint8_t *schema, uint32_t schema_len,
    uint8_t *request, uint64_t request_capacity, uint64_t *request_len);

/* Submit an encoded DELETE request on ABI v5's operation-aware optimizer
 * callback. The request uses only OptimizeDelete and grants no storage access. */
int32_t prdb_sdk_optimizer_submit_delete_v1(
    const prdb_host_api_v5 *host,
    const uint8_t *request, uint64_t request_len,
    uint8_t *response, uint64_t response_capacity, uint64_t *response_len,
    prdb_sdk_optimizer_response_v1 *out, uint32_t out_capacity);

#endif /* PRDB_SDK_OPTIMIZER_V2_H */
