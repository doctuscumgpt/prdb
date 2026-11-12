#include "prdb_query_abi_v3.h"
#include "prdb_query_abi_constants.h"
#include "prdb_query_abi_payload_codecs.h"

const prdb_query_descriptor_v3_record prdb_query_descriptor_v3 = {
    sizeof(prdb_query_descriptor_v3_record), PRDB_QUERY_ABI_V3_VERSION,
    sizeof(prdb_host_api_v3), sizeof(prdb_invocation_v3)};

int32_t prdb_query_main_v3(const prdb_host_api_v3 *host,
                           const prdb_invocation_v3 *invocation) {
    if (host == NULL || invocation == NULL ||
        host->struct_size < sizeof(*host) ||
        host->abi_version != PRDB_QUERY_ABI_V3_VERSION ||
        host->executor_request == NULL) {
        return 1;
    }

    uint8_t request[PRDB_EXECUTOR_STAGE_DELETE_REQUEST_HEADER_BYTES] = {0};
    uint8_t forged_handle[16] = {8};
    uint8_t response[PRDB_EXECUTOR_MUTATION_ACKNOWLEDGEMENT_HEADER_BYTES] = {0};
    uint64_t response_len = 0;
    prdb_encode_executor_stage_delete_request(request, forged_handle);

    /* Skip the SDK and try to delete through a valid-shaped, never-issued row
     * handle. The connection must reject it before staging any mutation. */
    int32_t status = host->executor_request(
        host->context, PRDB_IPC_OP_STAGE_DELETE, request, sizeof(request),
        response, sizeof(response), &response_len);
    return status == PRDB_QUERY_CALLBACK_OK ? 2 : 1;
}
