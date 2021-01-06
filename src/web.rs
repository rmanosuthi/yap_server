use std::net::SocketAddr;

use crate::imports::*;
use crate::symbols::*;

pub struct Web {}

pub type CoreAsker =
    tokio::sync::mpsc::Sender<(CoreRequest, tokio::sync::oneshot::Sender<Option<CoreReply>>)>;

impl Web {
    /**
    Main loop for Web. Warp handles the loop.

    Has a shutdown receiver.
    */
    pub async fn internal(
        nc: NetConfig,
        web_chans: WebChannels,
        r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
    ) {
        let mut lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let mut r_stop = r_stop;
        let mut web_chans = web_chans;
        let s_ws = web_chans.s_ws.clone();
        let ac = web_chans.ask_core.clone();
        let cls_lt_uid = lt_uid.clone();
        //let (s_webworker, r_from_webworker) = tokio::sync::mpsc::channel(1000);
        let login =
            warp::post()
                .and(warp::any().map(move || ac.clone()))
                .and(warp::any().map(move || s_ws.clone()))
                .and(warp::path("login"))
                .and(warp::body::json())
                .and(warp::any().map(move || cls_lt_uid.clone()))
                .and_then(async move |ask_core, notify_ws, login_req, lt_uid| {
                    match Web::try_auth_user(ask_core, login_req).await {
                        Some(uid) => Web::handle_login(lt_uid, notify_ws, uid).await,
                        None => Err(warp::reject::custom(WebInvalidUser)),
                    }
                });

        let ac = web_chans.ask_core.clone();
        let register = warp::post()
            .and(warp::any().map(move || ac.clone()))
            .and(warp::path("register"))
            .and(warp::body::json())
            .and_then(Web::handle_register);

        let ac = web_chans.ask_core.clone();
        let cls_lt_uid = lt_uid.clone();
        let userinfo = warp::get()
            .and(warp::any().map(move || ac.clone()))
            .and(warp::any().map(move || cls_lt_uid.clone()))
            .and(warp::path("users"))
            .and(warp::path::param::<UserId>())
            .and(warp::header::optional::<LoginToken>(
                http::header::AUTHORIZATION.as_str(),
            ))
            .and_then(Web::handle_userinfo);

        let (addr, server) = warp::serve(userinfo.or(login).or(register))
            .bind_with_graceful_shutdown(nc.api_addr.parse::<SocketAddr>().unwrap(), async move {
                r_stop.recv().await;
                r_stop.recv().await;
                info!("Web: stopped");
            });
        info!("Web: started");
        server.await;
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
        if let Some(CoreReply::Register(uid)) = Core::ask(ask_core, CoreRequest::Register(register_req)).await {
            debug!("web: register ok {}", &uid);
            Ok(warp::reply::json(&uid))
        } else {
            Err(warp::reject::custom(WebRegisterError {}))
        }
    }

    async fn try_auth_user(ask_core: CoreAsker, login_req: LoginRequest) -> Option<UserId> {
        if let Some(CoreReply::Login(uid)) = Core::ask(ask_core, CoreRequest::Login(login_req)).await {
            Some(uid)
        } else {
            None
        }
    }

    // let ws know to expect this lt
    async fn handle_login(
        lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>>,
        notify_ws: tokio::sync::mpsc::Sender<WebToWs>,
        uid: UserId,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        let mut notify_ws = notify_ws;
        let mut hmap = lt_uid.write().await;
        let lt = LoginToken::new();
        hmap.insert(lt.clone(), uid.clone());
        info!("web: uid {} logged in", &uid);
        match notify_ws.send(WebToWs::AddToken(uid, lt.clone())).await {
            Ok(_) => Ok(warp::reply::json(&lt)),
            Err(_) => Err(warp::reject::custom(WebChannelsError)),
        }
    }
}
