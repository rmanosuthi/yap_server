use chrono::Utc;
use common::*;
use futures::SinkExt;
use futures::stream::unfold;
use log::{debug, info, warn};
use std::{collections::HashMap, fmt::Display, net::SocketAddr, pin::pin};
use tokio::{
    net::TcpStream, runtime::{Handle, Runtime}, sync::{mpsc, oneshot}, task::{JoinHandle, JoinSet}
};
use tokio_stream::StreamExt;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{
    handshake::server::{Callback, Request, Response},
    http::{StatusCode, header},
};

use crate::{
    ServiceDone, data::WsServerboundTx, ipc::{CoreToWs, WebToWs, WorkerToWs, WsChannels, WsToCore, WsToWeb, WsToWorker}, net::NetConfig
};
use anyhow::{Error, Result};

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

enum WsEvent {
    FailedTcpSocket(Error),
    TcpSocket(TcpStream, SocketAddr),
    Web(WebToWs),
    Core(CoreToWs),
    Worker(WorkerToWs),
    Stop,
}

impl Ws {
    pub fn serve(hd: &Handle, js: &mut JoinSet<ServiceDone>, nc: NetConfig, chans: WsChannels) {
        js.spawn_on(async move {
            Ws::internal(nc, chans).await
        }, hd);
    }
    async fn internal(nc: NetConfig, chans: WsChannels) -> ServiceDone {
        let mut tcp_listener = tokio::net::TcpListener::bind(nc.ws_addr.to_owned()).await?;
        let mut run = true;
        let mut chans = chans;
        let mut cid_uid_lookup = HashMap::new();
        let mut uid_cids_lookup = HashMap::new();
        let mut lt_uid_lookup = HashMap::new();
        let mut s_workers = HashMap::new();
        let mut cia = ConnectionIdAllocator::new();
        let (mut s_to_worker, mut r_from_worker) = tokio::sync::mpsc::channel(100000);
        let mut first_stopped = false;
        let ev_new_tcp_conn = unfold(tcp_listener, |mut tcp_listener| async {
            Some((
                match tcp_listener.accept().await {
                    Ok((stream, addr)) => WsEvent::TcpSocket(stream, addr),
                    Err(e) => WsEvent::FailedTcpSocket(e.into()),
                },
                tcp_listener,
            ))
        });
        let ev_new_web_msg = unfold(chans.r_web, |mut r_web| async {
            r_web.recv().await.map(|m_web| (WsEvent::Web(m_web), r_web))
        });
        let ev_new_core_msg = unfold(chans.r_core, |mut r_core| async {
            r_core
                .recv()
                .await
                .map(|m_core| (WsEvent::Core(m_core), r_core))
        });
        let ev_new_worker_msg = unfold(r_from_worker, |mut r_worker| async {
            r_worker
                .recv()
                .await
                .map(|m_worker| (WsEvent::Worker(m_worker), r_worker))
        });
        let ev_r_stop = unfold(chans.r_stop, |mut r_stop| async {
            r_stop.changed().await.ok().map(|_| (WsEvent::Stop, r_stop))
        });
        let mut evs = pin!(
            ev_new_tcp_conn
                .merge(ev_new_web_msg)
                .merge(ev_new_core_msg)
                .merge(ev_new_worker_msg)
                .merge(ev_r_stop)
        );
        info!("Ws: started");
        info!("{:?}", &nc);
        while let Some(ev) = evs.next().await {
            debug!("WS LOOP");
            match ev {
                WsEvent::TcpSocket(stream, addr) => {
                    debug!("ws internal: new tcp connection {:?}", &stream);
                    Ws::handle_new_tcp(
                        &mut cia,
                        stream,
                        &chans.s_web,
                        &mut s_workers,
                        &mut cid_uid_lookup,
                        &mut uid_cids_lookup,
                        &mut lt_uid_lookup,
                        s_to_worker.clone(),
                    )
                    .await;
                    debug!("ws internal: new tcp success");
                }
                WsEvent::FailedTcpSocket(e) => {}
                WsEvent::Web(msg) => {
                    Ws::handle_web(
                        &mut cia,
                        msg,
                        &chans.s_web,
                        &mut s_workers,
                        &mut cid_uid_lookup,
                        &mut uid_cids_lookup,
                        &mut lt_uid_lookup,
                    )
                    .await;
                }
                WsEvent::Core(msg) => {
                    debug!("ws internal: new core msg {:?}", &msg);
                    match msg {
                        CoreToWs::SendDirect { dest, tx } => {
                            // get dest's connected devices
                            match uid_cids_lookup.get(&dest) {
                                Some(cids) => {
                                    let m = WsToWorker::from(tx);
                                    for cid in cids {
                                        debug!("ws internal: sending payload to cid {}", &cid);
                                        s_workers.get_mut(&cid).unwrap().send(m.clone()).await;
                                    }
                                }
                                None => {
                                    warn!("ws internal: send to uid {} failed, no devices", &dest);
                                }
                            }
                        }
                        _ => unimplemented!(),
                    }
                }
                WsEvent::Worker(msg) => {
                    debug!("ws internal: new worker msg {:?}", &msg);
                    match msg {
                        WorkerToWs::ForwardToCore(cid, tung_msg) => {
                            if let Some(uid) = cid_uid_lookup.get(&cid) {
                                match WsServerboundTx::new(uid.to_owned(), tung_msg)
                                    .map(WsToCore::from)
                                {
                                    Some(core_msg) => {
                                        chans.s_core.send(core_msg).await;
                                    }
                                    None => {}
                                }
                            } else {
                                warn!(
                                    "ws -> core: no associated uid with cid {} for sending string",
                                    &cid
                                );
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
                        }
                        _ => unimplemented!(),
                    }
                }
                WsEvent::Stop => {
                    info!("Ws: stopped");
                    break;
                }
            }
        }
        Ok(Utc::now())
    }

    async fn handle_web(
        cia: &mut ConnectionIdAllocator,
        m_web: WebToWs,
        s_web: &mpsc::Sender<WsToWeb>,
        s_workers: &mut HashMap<ConnectionId, mpsc::Sender<WsToWorker>>,
        cid_uid_lookup: &mut HashMap<ConnectionId, UserId>,
        uid_cids_lookup: &mut HashMap<UserId, Vec<ConnectionId>>,
        lt_uid_lookup: &mut HashMap<LoginToken, UserId>,
    ) -> Result<()> {
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
        s_web: &mpsc::Sender<WsToWeb>,
        s_workers: &mut HashMap<ConnectionId, mpsc::Sender<WsToWorker>>,
        cid_uid_lookup: &mut HashMap<ConnectionId, UserId>,
        uid_cids_lookup: &mut HashMap<UserId, Vec<ConnectionId>>,
        lt_uid_lookup: &HashMap<LoginToken, UserId>,
        s_worker: mpsc::Sender<WorkerToWs>,
    ) -> Option<(UserId, ConnectionId)> {
        let (send_c_uuid, recv_c_uuid) = oneshot::channel();
        let cb = |req: &Request, resp: Response| {
            let hdr = req.headers();
            if let Some(hv) = hdr.get(header::AUTHORIZATION) {
                if let Ok(tk) = hv.to_str() {
                    if let Some(c_uuid) = lt_uid_lookup.get(&LoginToken { tk: tk.to_owned() }) {
                        // valid user
                        match send_c_uuid.send(c_uuid.to_owned()) {
                            Ok(_) => Ok(resp),
                            Err(e) => Err(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Some("Channel send error".to_owned()))
                                .unwrap()),
                        }
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
        s: mpsc::Sender<WorkerToWs>,
        r: mpsc::Receiver<WsToWorker>,
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
                            WsToWorker::Tx(tx) => {
                                debug!("worker cid {}: received payload", &conn_id);
                                if let Some(tung_msg) = tx.extract() {
                                    c.send(tung_msg).await; // possible send failure
                                }
                            }
                        }
                    }
                    Some(raw_c_msg) = c.next() => {
                        debug!("ws worker: new from client");
                        if let Ok(c_msg) = raw_c_msg {
                            s.send(WorkerToWs::ForwardToCore(conn_id.to_owned(), c_msg)).await;
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
