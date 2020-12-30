use std::unimplemented;

use crate::imports::*;
use crate::symbols::*;

pub struct Core {}

#[derive(Serialize, Deserialize, Debug)]
pub struct CoreConfig {
    pub mysql_addr: String,
    pub mysql_port: u16,
    pub mysql_user: String,
    pub mysql_pass: String
}

impl Core {
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
    pub async fn internal(cc: CoreConfig, chans: CoreChannels) {
        let mut run = true;
        let mut r_f_ws = chans.r_ws;
        let mut s_t_ws = chans.s_ws;
        let mut r_stop = chans.r_stop;
        let mut r_corereq = chans.r_corereq;
        let mut store = Storage::new(
            &cc.mysql_addr,
            &cc.mysql_port.to_string(),
            &cc.mysql_user,
            &cc.mysql_pass
        ).unwrap();
        let mut first_stopped = false;
        info!("Core: started");
        while run {
            debug!("CORE LOOP");
            tokio::select! {
                Some(m_ws) = r_f_ws.recv() => {
                    let origin = m_ws.sender;
                    match m_ws.inner {
                        IncomingCoreMsg::String(stri) => {
                            debug!("CORE STR {:?} {}", &origin, &stri);
                            // parse as WsServerbound
                            if let Ok(wsm) = serde_json::from_str::<WsServerbound>(&stri) {
                                match wsm {
                                    WsServerbound::NewUserMessage {to, c} => {
                                        if let Some(umid) = store.new_message_u(origin.clone(), to.clone(), c.clone()) {
                                            s_t_ws.send(CoreToWs::SendDirect {
                                                src: origin.clone(),
                                                dest: to,
                                                msg: WsClientbound::NewUserMessage {
                                                        from: origin,
                                                        c,
                                                        umid
                                                    }
                                                
                                            }).await;
                                        }
                                    },
                                    _ => unimplemented!()
                                }
                            } else {
                                warn!("CORE INCOMING STR parse failed");
                            }
                        }
                        IncomingCoreMsg::Binary(u8s) => {
                            debug!("CORE BIN {:?} {:?}", &origin, &u8s);
                        }
                        IncomingCoreMsg::FlagRead(umid) => {
                            match store.flag_read_u(umid.clone()) {
                                Some(_) => {
                                    debug!("CORE flagged umid {} as read", &umid);
                                },
                                None => {
                                    warn!("CORE failed to flag umid {} as read", &umid);
                                }
                            }
                        }
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
    async fn handle_corereqs(creq: CoreRequest, store: &mut Storage) -> Option<CoreReply> {
        debug!("handling corereq {:?}", &creq);
        match creq {
            /*CoreRequest::GetClientId(lr) => store
                .try_login(&lr.email, &lr.password_hash)
                .await
                .map(|res| CoreReply::ClientId(res)),
            CoreRequest::Register(rr) => match store.register(rr).await {
                Ok(cid) => Some(CoreReply::Register(cid)),
                Err(_) => None,
            },
            CoreRequest::GetClientData(cid) => store
                .get_client(&cid)
                .await
                .map(|res| CoreReply::GetClientData(res)),
            CoreRequest::QueryMessage(q) => store
                .query(&q)
                .await
                .map(|res| CoreReply::QueryMessage(res)),*/
            CoreRequest::Login(req) => store.login(req).map(CoreReply::Login).ok(),
            CoreRequest::Register(req) => store.try_register(req).map(CoreReply::Register).ok(),
            CoreRequest::GetUserData(uid) => store.get_user_data(uid).map(CoreReply::GetUserData),
            CoreRequest::GetGroupData(gid) => store.get_group_data(gid).map(CoreReply::GetGroupData),
            CoreRequest::GetUserUserUnread { s, r } => store.get_user_user_unread(s, r).map(CoreReply::ClientBoundMessages),
            CoreRequest::GetUserGroupUnread { s, g } => store.get_user_group_unread(s, g).map(CoreReply::ClientBoundMessages),
            CoreRequest::NewUserMessage { u, d, c } => store.new_message_u(u, d, c).map(CoreReply::NewUserMessage),
            CoreRequest::NewGroupMessage { u, g, c } => store.new_message_g(u, g, c).map(CoreReply::NewGroupMessage),
            CoreRequest::GetUserLast { s, r, amt } => unimplemented!(),
            CoreRequest::GetGroupLast { s, g, amt } => unimplemented!()
        }
    }
}

#[derive(Debug)]
pub enum CoreRequest {
    Login(LoginRequest),
    Register(RegisterRequest),
    GetUserData(UserId),
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
    GetUserData(UserRecord),
    GetGroupData(GroupRecord),
    ClientBoundMessages(Vec<ServerMessage>),
    NewUserMessage(UserMessageId),
    NewGroupMessage(GroupMessageId),
}
