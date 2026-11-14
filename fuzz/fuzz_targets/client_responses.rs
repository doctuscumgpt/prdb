#![no_main]

use libfuzzer_sys::fuzz_target;
use prdb_engine::client_protocol::CLIENT_MAX_CONTROL_BYTES;
use prdb_engine::client_submit::fuzz_decode_bounded_response_json;

fuzz_target!(|data: &[u8]| {
    if data.len() <= CLIENT_MAX_CONTROL_BYTES {
        fuzz_decode_bounded_response_json(data);
    }
});
