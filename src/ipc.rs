use crate::core::CoreReply;
use crate::data::WsServerboundTx;
use crate::ws::ConnectionId;
use chrono::{DateTime, Utc};
use common::*;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{core::CoreRequest, data::WsClientboundTx, web::CoreAsker};

pub struct WsChannels {
    pub s_web: mpsc::Sender<WsToWeb>,
    pub r_web: mpsc::Receiver<WebToWs>,
    pub s_core: mpsc::Sender<WsToCore>,
    pub r_core: mpsc::Receiver<CoreToWs>,
    pub r_stop: watch::Receiver<DateTime<Utc>>,
}

pub struct WebChannels {
    pub s_ws: mpsc::Sender<WebToWs>,
    pub r_ws: mpsc::Receiver<WsToWeb>,
    pub r_stop: watch::Receiver<DateTime<Utc>>,
    pub ask_core: CoreAsker,
}

pub struct CoreChannels {
    pub s_ws: mpsc::Sender<CoreToWs>,
    pub r_ws: mpsc::Receiver<WsToCore>,
    pub r_stop: watch::Receiver<DateTime<Utc>>,
    pub r_corereq: mpsc::Receiver<(CoreRequest, oneshot::Sender<Option<CoreReply>>)>,
}

#[derive(Debug)]
pub enum WsToWeb {}

#[derive(Debug)]
pub enum WebToWs {
    AddToken(UserId, LoginToken),
    ClearTokens(UserId),
}

#[derive(Debug)]
pub enum CoreToWs {
    SendDirect {
        dest: UserId,
        tx: WsClientboundTx,
    },
    SendMultiple {
        dest: Vec<UserId>,
        tx: WsClientboundTx,
    },
}

impl CoreToWs {
    pub fn from_tx_u(dest: UserId, tx: WsClientboundTx) -> Self {
        Self::SendDirect { dest, tx }
    }
    pub fn from_tx_us(dest: Vec<UserId>, tx: WsClientboundTx) -> Self {
        Self::SendMultiple { dest, tx }
    }
}

#[derive(Debug)]
pub enum WsToCore {
    Tx(WsServerboundTx),
}

impl From<WsServerboundTx> for WsToCore {
    fn from(tx: WsServerboundTx) -> Self {
        Self::Tx(tx)
    }
}

#[derive(Debug)]
pub enum WorkerToWs {
    /// ws will intercept this, transform into `WsServerboundTx`, then forward to core.
    ForwardToCore(ConnectionId, tungstenite::Message),
    /// ws will remove the mapping to uid.
    Disconnected(ConnectionId),
    /// (unused) ws will let core know the umid has been sent successfully.
    ConfirmTransmittedUserMsg(ConnectionId, UserMessageId),
}

#[derive(Debug, Clone)]
pub enum WsToWorker {
    Tx(WsClientboundTx),
    Disconnect,
}

impl From<WsClientboundTx> for WsToWorker {
    fn from(tx: WsClientboundTx) -> Self {
        Self::Tx(tx)
    }
}
