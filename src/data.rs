use common::*;
use serde::{Deserialize, Serialize};

/** Message that is sent to the ws worker.*/
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WsClientboundTx {
    inner: WsClientboundPayload,
}

impl From<WsClientboundPayload> for WsClientboundTx {
    fn from(pl: WsClientboundPayload) -> Self {
        Self { inner: pl }
    }
}

impl WsClientboundTx {
    pub fn extract(self) -> Option<tungstenite::Message> {
        serde_json::to_string(&self.inner)
            .ok()
            .map(tungstenite::Message::from)
    }
}

#[derive(Debug)]
pub struct WsServerboundTx {
    sender: UserId,
    inner: WsServerboundPayload,
}

impl WsServerboundTx {
    /// Convert a raw ws message to a transmission.
    ///
    /// Deserialization should be done at entry point for performance reasons.
    pub fn new(sender: UserId, payload: tungstenite::Message) -> Option<WsServerboundTx> {
        match payload {
            tungstenite::Message::Text(s) => {
                serde_json::from_str(&s)
                    .ok()
                    .map(|deserialized| WsServerboundTx {
                        sender,
                        inner: deserialized,
                    })
            }
            _ => None,
        }
    }
    pub fn extract(self) -> (UserId, WsServerboundPayload) {
        (self.sender, self.inner)
    }
}

