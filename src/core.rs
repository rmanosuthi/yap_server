use std::unimplemented;

use crate::imports::*;
use crate::symbols::*;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub db_addr: String,
    pub api_addr: String,
    pub ws_addr: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CoreConfig {
    pub db_addr: String
}

impl From<&Config> for CoreConfig {
    fn from(c: &Config) -> Self {
        CoreConfig {db_addr: c.db_addr.clone()}
    }
}

pub struct Core {}

impl Core {
    /// Convenience function for asking core to process data.
    pub async fn ask(ca: CoreAsker, req: CoreRequest) -> Option<CoreReply> {
        let (s, r) = tokio::sync::oneshot::channel();
        let mut ca = ca;
        if let Ok(()) = ca.send((req, s)).await {
            if let Ok(cid_maybe) = r.await {
                debug!("coreask: reply received {:?}", &cid_maybe);
                cid_maybe
            } else {
                debug!("coreask: no reply");
                None
            }
        } else {
            debug!("coreask: send failed");
            None
        }
    }
    pub fn run(cc: CoreConfig, chans: CoreChannels) {
        let mut r_stop = chans.r_stop.clone();
        std::thread::spawn(move || {
            let mut rt = tokio::runtime::Builder::new()
                .threaded_scheduler()
                .enable_all()
                .build()
                .unwrap();
            rt.spawn(async move {
                Core::internal(cc, chans).await;
            });
            rt.block_on(async move {
                r_stop.recv().await.unwrap();
                r_stop.recv().await.unwrap();
                debug!("Core: stopped");
            });
        });
    }
    /**
    Main loop for Core. Does the following:
    - process inbound ws messages
    - process inbound core requests

    Has a shutdown receiver.
    */
    pub async fn internal(cc: CoreConfig, chans: CoreChannels) {
        let mut run = true;
        let mut r_f_ws = chans.r_ws;
        let mut s_t_ws = chans.s_ws;
        let mut r_stop = chans.r_stop;
        let mut r_corereq = chans.r_corereq;
        let mut store = Storage::new(
            &cc.db_addr
        )
        .unwrap();
        let mut first_stopped = false;
        info!("Core: started");
        while run {
            debug!("CORE LOOP");
            tokio::select! {
                Some(m_ws) = r_f_ws.recv() => {
                    match m_ws {
                        WsToCore::Tx(r_tx) => {
                            let (uid, r_tx) = r_tx.extract();
                            match r_tx {
                                WsServerboundPayload::NewUserMessage {to, content: c} => {
                                    if let Some(p_msg) = store.new_message_u(uid.clone(), to.clone(), c.clone()) {
                                        let send = Some(p_msg) // necessary to use map
                                            .map(WsClientboundPayload::from)
                                            .map(WsClientboundTx::from)
                                            .map(move |tx| CoreToWs::from_tx_u(to, tx))
                                            .unwrap();
                                        s_t_ws.send(send).await;
                                    }
                                },
                                _ => unimplemented!()
                            }
                        },
                        _ => unimplemented!()
                    }
                }
                Some(_) = r_stop.recv() => {
                    if !first_stopped {
                        first_stopped = true;
                        debug!("CORE set first stop");
                    } else {
                        debug!("Core: stopping");
                        run = false;
                        info!("Core: stopped");
                    }
                }
                Some((creq, s)) = r_corereq.recv() => {
                    debug!("core: received corereq");
                    s.send(Core::handle_corereqs(creq, &mut store).await);
                }
            }
        }
    }
    /**
    Logic for handling core requests.
    */
    async fn handle_corereqs(creq: CoreRequest, store: &mut Storage) -> Option<CoreReply> {
        debug!("handling corereq {:?}", &creq);
        match creq {
            CoreRequest::Login(req) => store.try_login(req).map(CoreReply::Login).ok(),
            CoreRequest::Register(req) => store.try_register(req).map(CoreReply::Register).ok(),
            CoreRequest::GetUserData { lookup, asker } => store
                .get_user_data(lookup, asker)
                .map(CoreReply::GetUserData),
            CoreRequest::GetGroupData(gid) => {
                store.get_group_data(gid).map(CoreReply::GetGroupData)
            }
            CoreRequest::GetUserUserUnread { s, r } => store
                .get_user_user_unread(s, r)
                .map(WsClientboundPayload::from)
                .map(WsClientboundTx::from)
                .map(CoreReply::ClientboundTx),
            CoreRequest::GetUserGroupUnread { s, g } => store
                .get_user_group_unread(s, g)
                .map(WsClientboundPayload::from)
                .map(WsClientboundTx::from)
                .map(CoreReply::ClientboundTx),
            CoreRequest::NewUserMessage { u, d, c } => {
                store.new_message_u(u, d, c).map(CoreReply::NewUserMessage)
            }
            CoreRequest::NewGroupMessage { u, g, c } => {
                store.new_message_g(u, g, c).map(CoreReply::NewGroupMessage)
            }
            CoreRequest::GetUserLast { s, r, amt } => unimplemented!(),
            CoreRequest::GetGroupLast { s, g, amt } => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub enum CoreRequest {
    Login(LoginRequest),
    Register(RegisterRequest),
    GetUserData {
        lookup: UserId,
        asker: Option<UserId>,
    },
    GetGroupData(GroupId),
    GetUserUserUnread {
        s: UserId,
        r: UserId,
    },
    GetUserGroupUnread {
        s: UserId,
        g: GroupId,
    },
    GetUserLast {
        s: UserId,
        r: UserId,
        amt: u16,
    },
    GetGroupLast {
        s: UserId,
        g: GroupId,
        amt: u16,
    },
    NewUserMessage {
        u: UserId,
        d: UserId,
        c: ClientMessage,
    },
    NewGroupMessage {
        u: UserId,
        g: GroupId,
        c: ClientMessage,
    },
}

#[derive(Debug)]
pub enum CoreReply {
    Login(UserId),
    Register(UserId),
    GetUserData(PublicUserRecord),
    GetGroupData(GroupRecord),
    ClientboundTx(WsClientboundTx),
    ClientboundTxs(Vec<WsClientboundTx>),
    NewUserMessage(PublicUserMessage),
    NewGroupMessage(GroupMessageId),
}
