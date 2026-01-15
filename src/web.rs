use anyhow::Result;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use warp::Filter;

use chrono::{DateTime, Utc};
use log::{debug, info};
use tokio::{
    runtime::Handle,
    sync::{RwLock, mpsc, watch},
    task::JoinSet,
};

use crate::{
    ServiceDone,
    core::{Core, CoreReply, CoreRequest},
    ipc::{WebChannels, WebToWs},
    net::NetConfig,
};
use common::*;
use tungstenite::http;
use warp::reject::Reject;

pub struct Web {}

pub type CoreAsker =
    tokio::sync::mpsc::Sender<(CoreRequest, tokio::sync::oneshot::Sender<Option<CoreReply>>)>;

#[derive(Debug)]
pub struct WebInvalidPassword;

impl Reject for WebInvalidPassword {}

#[derive(Debug)]
pub struct WebInvalidUser;

impl Reject for WebInvalidUser {}

#[derive(Debug)]
pub struct WebRegisterError {}

impl Reject for WebRegisterError {}

#[derive(Debug)]
pub struct WebChannelsError;

impl Reject for WebChannelsError {}

#[derive(Debug)]
pub struct WebCoreLookupFailed;

impl Reject for WebCoreLookupFailed {}

#[derive(Debug)]
pub struct WebInvalidLoginToken;

impl Reject for WebInvalidLoginToken {}

impl Web {
    pub fn serve(
        hd: &Handle,
        js: &mut JoinSet<ServiceDone>,
        nc: NetConfig,
        web_chans: WebChannels,
        r_stop: watch::Receiver<DateTime<Utc>>,
    ) {
        js.spawn_on(
            async move { Web::internal(nc, web_chans, r_stop).await },
            hd,
        );
    }
    /**
    Main loop for Web. Warp handles the loop.

    Has a shutdown receiver.
    */
    async fn internal(
        nc: NetConfig,
        web_chans: WebChannels,
        r_stop: watch::Receiver<DateTime<Utc>>,
    ) -> ServiceDone {
        // WORKAROUND: https://github.com/seanmonstar/warp/issues/1130
        // &'static str -> String
        let lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let mut r_stop = r_stop;
        let s_ws = web_chans.s_ws.clone();
        let ac = web_chans.ask_core.clone();
        let cls_lt_uid = lt_uid.clone();
        let ws_url = nc.ws_addr;
        let login =
            warp::post()
                .and(warp::any().map(move || ac.clone()))
                .and(warp::any().map(move || s_ws.clone()))
                .and(warp::any().map(move || ws_url.to_owned()))
                .and(warp::path("login".to_string()))
                .and(warp::body::json())
                .and(warp::any().map(move || cls_lt_uid.clone()))
                .and_then(async move |ask_core, notify_ws, ws_url, login_req, lt_uid| {
                    match Web::try_auth_user(ask_core, login_req).await {
                        Some(uid) => Web::handle_login(ws_url, lt_uid, notify_ws, uid).await,
                        None => Err(warp::reject::custom(WebInvalidUser)),
                    }
                });

        let ac = web_chans.ask_core.clone();
        let register = warp::post()
            .and(warp::any().map(move || ac.clone()))
            .and(warp::path("register".to_string()))
            .and(warp::body::json())
            .and_then(Web::handle_register);

        let ac = web_chans.ask_core.clone();
        let cls_lt_uid = lt_uid.clone();
        let userinfo = warp::get()
            .and(warp::any().map(move || ac.clone()))
            .and(warp::any().map(move || cls_lt_uid.clone()))
            .and(warp::path("users".to_string()))
            .and(warp::path::param::<UserId>())
            .and(warp::header::optional::<LoginToken>("Authorization"))
            .and_then(Web::handle_userinfo);

        let server = warp::serve(userinfo.or(login).or(register))
            .bind(nc.api_sock_addr)
            .await
            .graceful(async move {
                r_stop.changed().await;
                info!("Web: stopped");
            });
        info!("Web: started");
        server.run().await;
        Ok(Utc::now())
    }

    async fn handle_userinfo(
        ca: CoreAsker,
        lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>>,
        access_uid: UserId,
        auth_opt: Option<LoginToken>,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        match auth_opt {
            Some(lt) => match lt_uid.read().await.get(&lt) {
                Some(associated_uid) => {
                    if let Some(CoreReply::GetUserData(pur)) = Core::ask(
                        ca,
                        CoreRequest::GetUserData {
                            lookup: access_uid,
                            asker: Some(associated_uid.to_owned()),
                        },
                    )
                    .await
                    {
                        Ok(warp::reply::json(&pur))
                    } else {
                        Err(warp::reject::custom(WebCoreLookupFailed))
                    }
                }
                None => {
                    // invalid lt
                    Err(warp::reject::custom(WebInvalidLoginToken {}))
                }
            },
            None => {
                // unprivileged access
                // visibility public
                if let Some(CoreReply::GetUserData(pur)) = Core::ask(
                    ca,
                    CoreRequest::GetUserData {
                        lookup: access_uid,
                        asker: None,
                    },
                )
                .await
                {
                    Ok(warp::reply::json(&pur))
                } else {
                    Err(warp::reject::custom(WebCoreLookupFailed {}))
                }
            }
        }
    }

    async fn handle_register(
        ask_core: CoreAsker,
        register_req: RegisterRequest,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        debug!("web: register request {:?} received", &register_req);
        if let Some(CoreReply::Register(uid)) =
            Core::ask(ask_core, CoreRequest::Register(register_req)).await
        {
            debug!("web: register ok {}", &uid);
            Ok(warp::reply::json(&uid))
        } else {
            Err(warp::reject::custom(WebRegisterError {}))
        }
    }

    async fn try_auth_user(ask_core: CoreAsker, login_req: LoginRequest) -> Option<UserId> {
        if let Some(CoreReply::Login(uid)) =
            Core::ask(ask_core, CoreRequest::Login(login_req)).await
        {
            Some(uid)
        } else {
            None
        }
    }

    // let ws know to expect this lt
    async fn handle_login(
        ws_url: String,
        lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>>,
        notify_ws: mpsc::Sender<WebToWs>,
        uid: UserId,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        let mut notify_ws = notify_ws;
        let mut hmap = lt_uid.write().await;
        let lt = LoginToken::new();
        hmap.insert(lt.clone(), uid.clone());
        info!("web: uid {} logged in", &uid);
        match notify_ws.send(WebToWs::AddToken(uid, lt.clone())).await {
            Ok(_) => {
                let reply = warp::reply::json(&lt);
                let reply = warp::reply::with_header(reply, "Yap-Ws-Url", ws_url);
                Ok(reply)
            }
            Err(_) => Err(warp::reject::custom(WebChannelsError)),
        }
    }
}
