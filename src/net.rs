use tokio::join;

use crate::imports::*;
use crate::symbols::*;

pub struct Net {
    s_pause: Sender<chrono::DateTime<chrono::Utc>>,
}

impl Net {
    pub fn make_chans(
        r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
    ) -> (WebChannels, WsChannels, CoreChannels) {
        let (s_web_ws, r_web_ws) = mpsc::channel(1000);
        let (s_ws_web, r_ws_web) = mpsc::channel(1000);
        let (s_core_ws, r_core_ws) = mpsc::channel(1000);
        let (s_ws_core, r_ws_core) = mpsc::channel(1000);
        let (corereq, r_corereq) = mpsc::channel(1000);
        (
            WebChannels {
                s_ws: s_web_ws,
                r_ws: r_ws_web,
                r_stop: r_stop.clone(),
                ask_core: corereq,
            },
            WsChannels {
                s_web: s_ws_web,
                r_web: r_web_ws,
                s_core: s_ws_core,
                r_core: r_core_ws,
                r_stop: r_stop.clone(),
            },
            CoreChannels {
                s_ws: s_core_ws,
                r_ws: r_ws_core,
                r_stop: r_stop.clone(),
                r_corereq,
            },
        )
    }
    pub fn resume(&self) {}
    pub fn pause(&self) {}
    pub fn build(
        nc: NetConfig,
        web_chans: WebChannels,
        ws_chans: WsChannels,
        r_stop: tokio::sync::watch::Receiver<DateTime<Utc>>,
    ) -> Result<Net, Box<dyn Error>> {
        info!("building net...");
        let mut r_stop = r_stop;
        let (s_pause, r_pause) = tokio::sync::mpsc::channel(10);
        let nc_closure = nc.clone();
        std::thread::spawn(move || {
            let mut rt = tokio::runtime::Builder::new()
                .threaded_scheduler()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                tokio::task::spawn(async move {
                    Ws::internal(nc.clone(), ws_chans).await;
                });
                let nc = nc_closure.clone();
                let r_stop_closure = r_stop.clone();
                tokio::task::spawn(async move {
                    Web::internal(nc, web_chans, r_stop_closure).await;
                });
                info!("Net: started");
                r_stop.recv().await.unwrap();
                r_stop.recv().await.unwrap();
                info!("Net: stopped");
            });
        });
        info!("net built!");
        Ok(Net { s_pause })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetConfig {
    pub api_addr: String,
    pub ws_addr: String,
    pub enable_register: bool,
}

impl From<&Config> for NetConfig {
    fn from(c: &Config) -> Self {
        NetConfig {
            api_addr: c.api_addr.clone(),
            ws_addr: c.ws_addr.clone(),
            enable_register: true
        }
    }
}