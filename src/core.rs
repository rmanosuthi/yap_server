use std::pin::pin;

use crate::ServiceDone;
use crate::data::WsClientboundTx;
use crate::ipc::*;
use crate::storage::*;
use crate::web::*;
use chrono::Utc;
use common::*;
use futures::stream::unfold;
use log::debug;
use log::info;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use tokio_stream::StreamExt;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub db_addr: String,
    pub api_addr: String,
    pub ws_addr: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CoreConfig {
    pub db_addr: String,
}

impl From<&Config> for CoreConfig {
    fn from(c: &Config) -> Self {
        CoreConfig {
            db_addr: c.db_addr.clone(),
        }
    }
}

pub struct Core {}

enum CoreEvent<T: Sized> {
    MWsToCore(WsToCore),
    Stop,
    Request(T),
}

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
    pub fn serve(hd: &Handle, js: &mut JoinSet<ServiceDone>, cc: CoreConfig, chans: CoreChannels) -> usize {
        js.spawn_on(async move {
            Core::internal(cc, chans).await
        }, hd);
        1
    }
    /**
    Main loop for Core. Does the following:
    - process inbound ws messages
    - process inbound core requests

    Has a shutdown receiver.
    */
    async fn internal(cc: CoreConfig, chans: CoreChannels) -> ServiceDone {
        let mut store = Storage::new(&cc.db_addr)?;
        let ev_recv_ws_to_core = unfold(chans.r_ws, |mut r_f_ws| async {
            r_f_ws
                .recv()
                .await
                .map(|m| (CoreEvent::MWsToCore(m), r_f_ws))
        });
        let ev_recv_corereq = unfold(chans.r_corereq, |mut r_corereq| async {
            r_corereq
                .recv()
                .await
                .map(|m| (CoreEvent::Request(m), r_corereq))
        });
        let ev_r_stop = unfold(chans.r_stop, |mut r_stop| async {
            r_stop
                .changed()
                .await
                .ok()
                .map(|_| (CoreEvent::Stop, r_stop))
        });
        let mut evs = pin!(ev_recv_ws_to_core.merge(ev_recv_corereq).merge(ev_r_stop));
        info!("Core: started");
        while let Some(ev) = evs.next().await {
            debug!("CORE LOOP");
            match ev {
                CoreEvent::MWsToCore(WsToCore::Tx(r_tx)) => {
                    let (uid, r_tx) = r_tx.extract();
                    match r_tx {
                        WsServerboundPayload::NewUserMessage { to, content: c } => {
                            if let Some(p_msg) =
                                store.new_message_u(uid.clone(), to.clone(), c.clone())
                            {
                                let send = Some(p_msg) // necessary to use map
                                            .map(WsClientboundPayload::from)
                                            .map(WsClientboundTx::from)
                                            .map(move |tx| CoreToWs::from_tx_u(to, tx))
                                            .unwrap();
                                chans.s_ws.send(send).await;
                            }
                        }
                    }
                }
                CoreEvent::Request((creq, s)) => {
                    debug!("core: received corereq");
                    s.send(Core::handle_corereqs(creq, &mut store).await);
                }
                CoreEvent::Stop => {
                    debug!("Core: stopping");
                    break;
                }
            }
        }
        info!("Core: stopped");
        Ok(Utc::now())
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
