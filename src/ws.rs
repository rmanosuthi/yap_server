use hashbrown::hash_map::Entry;

use crate::imports::*;
use crate::symbols::*;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ConnectionId(pub u64);

impl Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "connection ({})", self.0)
    }
}

pub struct Ws {}

pub struct ConnectionIdAllocator {
    inner: u64,
}

impl ConnectionIdAllocator {
    pub fn new() -> ConnectionIdAllocator {
        ConnectionIdAllocator { inner: 0 }
    }
    pub fn get(&mut self) -> ConnectionId {
        let res = ConnectionId(self.inner);
        self.inner += 1;
        res
    }
}

impl Ws {
    pub async fn internal(nc: NetConfig, chans: WsChannels) -> Result<(), NetInternalError> {
        let mut tcp_listener = tokio::net::TcpListener::bind(nc.ws_addr.to_owned())
            .await
            .map_err(|e| NetInternalError::ListenerBind(Box::new(e)))?;
        let mut run = true;
        let mut chans = chans;
        let mut cid_uid_lookup = HashMap::new();
        let mut uid_cids_lookup = HashMap::new();
        let mut lt_uid_lookup = HashMap::new();
        let mut s_workers = HashMap::new();
        let mut cia = ConnectionIdAllocator::new();
        let (mut s_to_worker, mut r_from_worker) = tokio::sync::mpsc::channel(100000);
        let mut first_stopped = false;
        info!("Ws: started");
        info!("{:?}", &nc);
        while run {
            tokio::select! {
                Some(ntc_maybe) = tcp_listener.next() => {
                    if let Ok(ntc) = ntc_maybe {
                        debug!("ws internal: new tcp connection {:?}", &ntc);
                        Ws::handle_new_tcp(
                            &mut cia,
                            ntc,
                            &chans.s_web,
                            &mut s_workers,
                            &mut cid_uid_lookup,
                            &mut uid_cids_lookup,
                            &mut lt_uid_lookup,
                            s_to_worker.clone()
                        ).await;
                        debug!("ws internal: new tcp success");
                    }
                }
                Some(m_web) = chans.r_web.recv() => {
                    debug!("ws internal: new web msg {:?}", &m_web);
                    Ws::handle_web(
                        &mut cia,
                        m_web,
                        &chans.s_web,
                        &mut s_workers,
                        &mut cid_uid_lookup,
                        &mut uid_cids_lookup,
                        &mut lt_uid_lookup,
                    ).await;
                }
                Some(m_core) = chans.r_core.recv() => {
                    debug!("ws internal: new core msg {:?}", &m_core);
                    match m_core {
                        CoreToWs::SendDirect {src, dest, msg: ws_c} => {
                            // get dest's connected devices
                            match uid_cids_lookup.get(&dest) {
                                Some(cids) => {
                                    for cid in cids {
                                        debug!("ws internal: sending payload to cid {}", &cid);
                                        s_workers.get_mut(&cid).unwrap().send(WsToWorker::Payload(ws_c.clone())).await;
                                    }
                                },
                                None => {
                                    warn!("ws internal: send to uid {} failed, no devices", &dest);
                                }
                            }
                        },
                        _ => unimplemented!()
                    }
                }
                Some(m_worker) = r_from_worker.recv() => {
                    debug!("ws internal: new worker msg {:?}", &m_worker);
                    match m_worker {
                        WorkerToWs::ForwardToCoreString(cid, stri) => {
                            if let Some(uid) = cid_uid_lookup.get(&cid) {
                                chans.s_core.send(WsToCore::new(uid.to_owned(), IncomingCoreMsg::String(stri))).await;
                            } else {
                                warn!("ws -> core: no associated uid with cid {} for sending string", &cid);
                            }
                        }
                        WorkerToWs::ForwardToCoreVec(cid, u8s) => {
                            if let Some(uid) = cid_uid_lookup.get(&cid) {
                                chans.s_core.send(WsToCore::new(uid.to_owned(), IncomingCoreMsg::Binary(u8s))).await;
                            } else {
                                warn!("ws -> core: no associated uid with cid {} for sending vec", &cid);
                            }
                        }
                        WorkerToWs::Disconnected(cid) => {
                            if let Some(uid) = cid_uid_lookup.get(&cid) {
                                let uid = uid.clone();
                                s_workers.retain(|k, v| *k != cid);
                                cid_uid_lookup.retain(|k, v| *k != cid);
                                uid_cids_lookup.get_mut(&uid).unwrap().retain(|e| *e != cid);
                            } else {
                                warn!("ws internal: received disconnect from unknown uid worker");
                            }
                        },
                        WorkerToWs::ConfirmTransmittedUserMsg(cid, umid) => {
                            if let Some(uid) = cid_uid_lookup.get(&cid) {
                                chans.s_core.send(WsToCore::new(
                                    uid.to_owned(),
                                    IncomingCoreMsg::FlagRead(umid)
                                )).await;
                            } else {
                                warn!("ws -> core: no associated uid with cid {} for confirm transmission", &cid);
                            }
                        }
                    }
                }
                Some(_) = chans.r_stop.recv() => {
                    if !first_stopped {
                        first_stopped = true;
                        debug!("Ws: set first stop");
                    } else {
                        run = false;
                        info!("Ws: stopped");
                    }
                }
            };
        }
        Ok(())
    }

    async fn handle_web(
        cia: &mut ConnectionIdAllocator,
        m_web: WebToWs,
        s_web: &Sender<WsToWeb>,
        s_workers: &mut HashMap<ConnectionId, Sender<WsToWorker>>,
        cid_uid_lookup: &mut HashMap<ConnectionId, UserId>,
        uid_cids_lookup: &mut HashMap<UserId, Vec<ConnectionId>>,
        lt_uid_lookup: &mut HashMap<LoginToken, UserId>,
    ) -> Result<(), Box<dyn Error>> {
        match m_web {
            WebToWs::AddToken(cid, lt) => {
                lt_uid_lookup.insert(lt, cid);
                Ok(())
            }
            // disconnect all connections
            WebToWs::ClearTokens(cid) => {
                info!("clearing and disconnecting {}", &cid);
                if let Some(conn_ids) = uid_cids_lookup.get(&cid) {
                    let disconnects_ks = s_workers
                        .iter()
                        .filter_map(|(k, _)| {
                            if conn_ids.contains(k) {
                                Some(k.to_owned())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<ConnectionId>>();
                    for k in disconnects_ks {
                        s_workers
                            .get_mut(&k)
                            .unwrap()
                            .send(WsToWorker::Disconnect)
                            .await;
                    }
                    s_workers.retain(|k, v| !conn_ids.contains(k));
                    cid_uid_lookup.retain(|k, v| !conn_ids.contains(k));
                    uid_cids_lookup.retain(|k, v| *k != cid);
                    lt_uid_lookup.retain(|k, v| *v != cid);
                    Ok(())
                } else {
                    // clear tokens failed, invalid c_uuid
                    Ok(())
                }
            }
        }
    }

    async fn handle_new_tcp(
        cia: &mut ConnectionIdAllocator,
        t: TcpStream,
        s_web: &Sender<WsToWeb>,
        s_workers: &mut HashMap<ConnectionId, Sender<WsToWorker>>,
        cid_uid_lookup: &mut HashMap<ConnectionId, UserId>,
        uid_cids_lookup: &mut HashMap<UserId, Vec<ConnectionId>>,
        lt_uid_lookup: &HashMap<LoginToken, UserId>,
        s_worker: Sender<WorkerToWs>,
    ) -> Option<(UserId, ConnectionId)> {
        let (send_c_uuid, recv_c_uuid) = tokio::sync::oneshot::channel();
        let cb = |req: &tungstenite::handshake::server::Request,
                  resp: Response<()>|
         -> Result<Response<()>, Response<Option<String>>> {
            let hdr = req.headers();
            if let Some(hv) = hdr.get(http::header::AUTHORIZATION) {
                if let Ok(tk) = hv.to_str() {
                    if let Some(c_uuid) = lt_uid_lookup.get(&LoginToken { tk: tk.to_owned() }) {
                        // valid user
                        send_c_uuid.send(c_uuid.to_owned()).map_err(|e| {
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Some("Channel send error".to_owned()))
                                .unwrap()
                        })?;
                        Ok(resp)
                    } else {
                        // unauthorized
                        Err(Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .body(Some("Unauthorized token".to_owned()))
                            .unwrap())
                    }
                } else {
                    Err(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Some("Token not a valid ASCII sequence".to_owned()))
                        .unwrap())
                }
            } else {
                Err(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Some("Missing authorization header".to_owned()))
                    .unwrap())
            }
        };
        match tokio_tungstenite::accept_hdr_async(t, cb).await {
            Ok(ws) => {
                let c_uuid = recv_c_uuid.await.unwrap();
                let cid = cia.get();

                info!("accepting new connection {}", &cid);

                let (w_s, r_w) = tokio::sync::mpsc::channel(1000);

                Ws::new_worker_conn(s_worker.clone(), r_w, ws, cid.clone()).await;

                s_workers.insert(cid.clone(), w_s);

                cid_uid_lookup.insert(cid.clone(), c_uuid.clone());

                if let Some(cids) = uid_cids_lookup.get_mut(&c_uuid) {
                    cids.push(cid.clone());
                } else {
                    uid_cids_lookup.insert(c_uuid.clone(), vec![cid.clone()]);
                }
                info!("ws: new tcp handle done");
                Some((c_uuid, cid))
            }
            Err(e) => None,
        }
    }

    async fn new_worker_conn(
        s: Sender<WorkerToWs>,
        r: Receiver<WsToWorker>,
        c: WebSocketStream<TcpStream>,
        conn_id: ConnectionId,
    ) {
        tokio::spawn(async move {
            debug!("NEW {}", &conn_id);
            let mut s = s;
            let mut r = r;
            let mut c = c;
            let mut worker_active = true;
            while worker_active {
                debug!("ws worker: active loop");
                //c.send(tungstenite::Message::Text("")).await;
                tokio::select! {
                    Some(ws_msg) = r.recv() => {
                        debug!("ws worker: new from ws");
                        match ws_msg {
                            WsToWorker::Disconnect => {
                                info!("{} disconnect received", &conn_id);
                                worker_active = false;
                                c.close(None).await;
                                s.send(WorkerToWs::Disconnected(conn_id.clone())).await;
                            },
                            WsToWorker::Payload(ws_c) => {
                                debug!("worker cid {}: received payload", &conn_id);
                                // serialize
                                match serde_json::to_string(&ws_c) {
                                    Ok(serialized_payload) => {
                                        // check if confirmation is necessary
                                        match c.send(tungstenite::Message::text(serialized_payload)).await {
                                            Ok(_) => {
                                                match ws_c {
                                                    WsClientbound::NewUserMessage {from: _, c: _, umid} => {
                                                        debug!("worker cid {}: transmit ok", &conn_id);
                                                        s.send(WorkerToWs::ConfirmTransmittedUserMsg(conn_id.to_owned(), umid)).await;
                                                    },
                                                    WsClientbound::NewGroupMessage {from: _, c: _, gmid} => {
                                                        unimplemented!();
                                                        debug!("worker cid {}: transmit ok", &conn_id);
                                                        //s.send().await;
                                                    },
                                                    _ => {}
                                                }
                                            },
                                            Err(e) => {
                                                error!("worker cid {}: transmit failed {:?}", &conn_id, e);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        error!("worker cid {}: payload serialization failed {:?}", &conn_id, e);
                                    }
                                }
                            }
                        }
                    }
                    Some(raw_c_msg) = c.next() => {
                        debug!("ws worker: new from client");
                        if let Ok(c_msg) = raw_c_msg {
                            match c_msg {
                                tungstenite::Message::Text(stri) => {
                                    // forward to core
                                    debug!("{} forwarding msg {}", &conn_id, &stri);
                                    s.send(WorkerToWs::ForwardToCoreString(conn_id.to_owned(), stri)).await;
                                }
                                tungstenite::Message::Binary(u8s) => {
                                    warn!("{} unexpected ws binary message {:?}", &conn_id, &u8s);
                                }
                                tungstenite::Message::Ping(ping) => {
                                    warn!("{} unexpected ws ping {:?}", &conn_id, &ping);
                                }
                                tungstenite::Message::Pong(pong) => {
                                    warn!("{} unexpected ws pong {:?}", &conn_id, &pong);
                                }
                                tungstenite::Message::Close(close_frame) => {
                                    info!("{} closing connection", &conn_id);
                                    worker_active = false;
                                    c.close(None).await;
                                    s.send(WorkerToWs::Disconnected(conn_id.clone())).await;
                                }
                            }
                        } else {
                            warn!("{} msg read failed", &conn_id);
                            worker_active = false;
                            c.close(None).await;
                            s.send(WorkerToWs::Disconnected(conn_id.clone())).await;
                        }
                    }
                }
            }
        });
    }
}
