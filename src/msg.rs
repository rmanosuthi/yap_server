mod imports {
    pub use crate::imports::*;
    pub use crate::symbols::*;
}
pub use self::channels::*;
use self::imports::*;
pub use self::msg::*;

mod channels {
    use super::imports::*;
    pub struct WsChannels {
        pub s_web: Sender<WsToWeb>,
        pub r_web: Receiver<WebToWs>,
        pub s_core: Sender<WsToCore>,
        pub r_core: Receiver<CoreToWs>,
        pub r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
    }

    pub struct WebChannels {
        pub s_ws: Sender<WebToWs>,
        pub r_ws: Receiver<WsToWeb>,
        pub r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
        pub ask_core: CoreAsker,
    }

    pub struct CoreChannels {
        pub s_ws: Sender<CoreToWs>,
        pub r_ws: Receiver<WsToCore>,
        pub r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
        pub r_corereq: Receiver<(CoreRequest, tokio::sync::oneshot::Sender<Option<CoreReply>>)>,
    }
}

mod msg {
    use super::imports::*;
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
            Self::SendDirect {dest, tx}
        }
        pub fn from_tx_us(dest: Vec<UserId>, tx: WsClientboundTx) -> Self {
            Self::SendMultiple {dest, tx}
        }
    }
    #[derive(Debug)]
    pub enum WsToCore {
        Tx(WsServerboundTx)
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
        ConfirmTransmittedUserMsg(ConnectionId, UserMessageId)
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
}
