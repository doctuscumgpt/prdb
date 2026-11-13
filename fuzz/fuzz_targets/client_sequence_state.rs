#![no_main]

use libfuzzer_sys::fuzz_target;
use prdb_engine::client_protocol::{ClientFrame, ClientRequestIds, ClientResponseSequences};

const EVENT_BYTES: usize = 19;

fuzz_target!(|data: &[u8]| {
    let mut request_ids = ClientRequestIds::default();
    let mut response_sequences = ClientResponseSequences::default();

    for event in data.chunks_exact(EVENT_BYTES).take(4096) {
        let request_id = u64::from_le_bytes(event[1..9].try_into().expect("fixed event width"));
        let kind = u16::from_le_bytes(event[9..11].try_into().expect("fixed event width"));
        let response_sequence =
            u64::from_le_bytes(event[11..19].try_into().expect("fixed event width"));
        let frame = ClientFrame {
            kind,
            request_id,
            response_sequence,
            payload: Vec::new(),
        };

        match event[0] % 3 {
            0 => {
                let _ = request_ids.accept(&frame);
            }
            1 => {
                let _ = response_sequences.begin_stream(request_id);
            }
            _ => {
                let _ = response_sequences.accept_stream_frame(&frame);
            }
        }
    }
});
