//! IPC frame decoding on arbitrary byte streams: the length-prefixed reader (several frames in
//! a row, ending mid-frame or on an oversized length) and the JSON decoders for what clients
//! and the engine send. A frame that decodes must survive an encode/decode round trip.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use tokio::runtime::{Builder, Runtime};
use packie_ipc::frame::{read_frame, read_json, write_json};
use packie_ipc::{ClientFrame, ServerFrame};

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_current_thread()
            .build()
            .expect("a current-thread runtime starts")
    })
}

async fn round_trip<T>(stream: &[u8])
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let mut reader = stream;
    while let Ok(Some(frame)) = read_json::<T, _>(&mut reader).await {
        let mut wire = Vec::new();
        write_json(&mut wire, &frame)
            .await
            .expect("a decoded frame encodes");
        let mut again = wire.as_slice();
        let decoded = read_json::<T, _>(&mut again).await;
        assert_eq!(decoded.expect("re-read").as_ref(), Some(&frame));
    }
}

fuzz_target!(|data: &[u8]| {
    runtime().block_on(async {
        let mut reader = data;
        while let Ok(Some(body)) = read_frame(&mut reader).await {
            assert!(body.len() <= data.len());
        }
        round_trip::<ClientFrame>(data).await;
        round_trip::<ServerFrame>(data).await;
        // A raw JSON body behind a correct length prefix reaches the decoders directly.
        let Ok(len) = u32::try_from(data.len()) else {
            return;
        };
        let mut framed = len.to_be_bytes().to_vec();
        framed.extend_from_slice(data);
        round_trip::<ClientFrame>(&framed).await;
        round_trip::<ServerFrame>(&framed).await;
    });
});
