#![no_main]

use libfuzzer_sys::fuzz_target;
use prdb_engine::client_protocol::{
    CLIENT_FRAME_HEADER_BYTES, CLIENT_MAX_PAYLOAD_BYTES, ClientFrameDecoder,
    decode_client_admin_command, decode_client_admin_read, decode_client_chunk,
    decode_client_credentials, decode_client_hello, decode_client_prepare, decode_client_seal,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > CLIENT_MAX_PAYLOAD_BYTES + CLIENT_FRAME_HEADER_BYTES {
        return;
    }

    let _ = decode_client_hello(data);
    let _ = decode_client_credentials(data);
    let _ = decode_client_prepare(data);
    let _ = decode_client_seal(data);
    let _ = decode_client_admin_read(data);
    let _ = decode_client_admin_command(data);
    let _ = decode_client_chunk(data);

    let mut decoder = ClientFrameDecoder::default();
    let chunk_width = data.first().copied().unwrap_or(1) as usize % 31 + 1;
    for chunk in data.chunks(chunk_width) {
        if decoder.push(chunk).is_err() {
            break;
        }
    }
    let _ = decoder.finish();
});
