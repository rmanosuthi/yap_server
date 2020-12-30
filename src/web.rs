use std::net::SocketAddr;

use crate::imports::*;
use crate::symbols::*;

pub struct Web {}

pub type CoreAsker =
    tokio::sync::mpsc::Sender<(CoreRequest, tokio::sync::oneshot::Sender<Option<CoreReply>>)>;


impl Web {
    pub async fn internal(
        nc: NetConfig,
        web_chans: WebChannels,
        r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
    ) {
        let mut lt_uid: Arc<RwLock<HashMap<LoginToken, UserId>>> = Arc::new(RwLock::new(HashMap::new()));
        let mut r_stop = r_stop;
        let mut web_chans = web_chans;
        let s_ws = web_chans.s_ws.clone();
        let ac = web_chans.ask_core.clone();
        //let (s_webworker, r_from_webworker) = tokio::sync::mpsc::channel(1000);
        let login =
            warp::post()
                .and(warp::any().map(move || ac.clone()))
                .and(warp::any().map(move || s_ws.clone()))
                .and(warp::path("login"))
                .and(warp::body::json())
                .and(warp::any().map(move || lt_uid.clone()))
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

        let hw = warp::get()
            .and(warp::path("helloworld"))
            .map(move || "Hello world!");

        let (addr, server) = warp::serve(hw.or(login).or(register)).bind_with_graceful_shutdown(
            nc.api_addr.parse::<SocketAddr>().unwrap(),
            async move {
                r_stop.recv().await;
                r_stop.recv().await;
                info!("Web: stopped");
            },
        );
        info!("Web: started");
        server.await;
    }

    async fn handle_register(
        ask_core: CoreAsker,
        register_req: RegisterRequest,
    ) -> Result<impl warp::Reply, warp::Rejection> {
        debug!("web: register request {:?} received", &register_req);
        match {
            let (s, r) = tokio::sync::oneshot::channel();
            let mut ask_core = ask_core;
            if let Ok(()) = ask_core
                .send((CoreRequest::Register(register_req), s))
                .await
            {
                debug!("web: register ask core ok");
                if let Ok(cid_maybe) = r.await {
                    debug!("web: register core reply received {:?}", &cid_maybe);
                    cid_maybe
                } else {
                    debug!("web: register no core reply");
                    None
                }
            } else {
                debug!("web: register ask core failed");
                None
            }
        } {
            Some(_) => Ok("registered"),
            None => Err(warp::reject::custom(WebRegisterError {})),
        }
    }

    async fn try_auth_user(ask_core: CoreAsker, login_req: LoginRequest) -> Option<UserId> {
        let (s, r) = tokio::sync::oneshot::channel();
        let mut ask_core = ask_core;
        if let Ok(()) = ask_core
            .send((CoreRequest::Login(login_req), s))
            .await
        {
            if let Ok(Some(CoreReply::Login(uid))) = r.await {
                Some(uid)
            } else {
                None
            }
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
