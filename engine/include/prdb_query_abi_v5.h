#ifndef PRDB_QUERY_ABI_V5_H
#define PRDB_QUERY_ABI_V5_H

#include "prdb_query_abi_v4.h"

/* ABI v5 exposes a bounded optimizer callback whose operation is selected by
 * the SDK request path. The host still applies the provisioned role allowlist;
 * this callback grants no storage authority. */
typedef int32_t (*prdb_query_optimizer_request_v5_fn)(
    void *context, uint16_t operation, const uint8_t *request,
    uint64_t request_len, uint8_t *response, uint64_t response_capacity,
    uint64_t *response_len);

#include "prdb_query_abi_records_v5.h"

typedef int32_t (*prdb_query_main_v5_fn)(const prdb_host_api_v5 *host,
                                         const prdb_invocation_v5 *invocation);

extern const prdb_query_descriptor_v5_record prdb_query_descriptor_v5;
int32_t prdb_query_main_v5(const prdb_host_api_v5 *host,
                           const prdb_invocation_v5 *invocation);

#endif /* PRDB_QUERY_ABI_V5_H */
