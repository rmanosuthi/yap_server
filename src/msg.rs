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
            src: UserId,
            dest: UserId,
            msg: WsClientbound,
        },
        SendMultiple {
            src: UserId,
            dest: Vec<UserId>,
            msg: WsClientbound,
        },
    }

    #[derive(Debug)]
    pub struct WsToCore {
        pub sender: UserId,
        pub inner: IncomingCoreMsg,
    }

    impl WsToCore {
        pub fn new(sender: UserId, inner: IncomingCoreMsg) -> WsToCore {
            WsToCore { sender, inner }
        }
    }

    #[derive(Debug)]
    pub enum IncomingCoreMsg {
        String(String),
        Binary(Vec<u8>),
        FlagRead(UserMessageId)
    }

    #[derive(Debug)]
    pub enum WorkerToWs {
        ForwardToCoreString(ConnectionId, String),
        ForwardToCoreVec(ConnectionId, Vec<u8>),
        Disconnected(ConnectionId),
        ConfirmTransmittedUserMsg(ConnectionId, UserMessageId)
    }

    #[derive(Debug)]
    pub enum WsToWorker {
        Payload(WsClientbound),
        Disconnect,
    }
}
