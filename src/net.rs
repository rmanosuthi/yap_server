use std::net::SocketAddr;
use std::net::ToSocketAddrs;

use crate::ServiceDone;
use crate::core::Config;
use crate::web::Web;
use crate::ws::Ws;
use anyhow::Error;
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::error;
use log::info;
use serde::Deserialize;
use serde::Serialize;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;

use crate::ipc::{CoreChannels, WebChannels, WsChannels};

pub struct Net {
    s_pause: mpsc::Sender<chrono::DateTime<chrono::Utc>>,
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
    pub fn serve(
        hd: &Handle,
        js: &mut JoinSet<ServiceDone>,
        nc: NetConfig,
        web_chans: WebChannels,
        ws_chans: WsChannels,
        r_stop: watch::Receiver<DateTime<Utc>>,
    ) -> usize {
        info!("building net...");
        Ws::serve(hd, js, nc.clone(), ws_chans);
        Web::serve(hd, js, nc, web_chans, r_stop);
        info!("net built!");
        return 2;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetConfig {
    pub api_sock_addr: SocketAddr,
    pub ws_addr: String,
    pub enable_register: bool,
}

impl TryFrom<&Config> for NetConfig {
    type Error = Error;

    fn try_from(c: &Config) -> std::result::Result<Self, Self::Error> {
        let api_sock_addrs = c.api_addr.to_socket_addrs()?.collect::<Vec<_>>();
        Ok(NetConfig {
            api_sock_addr: api_sock_addrs[0],
            ws_addr: c.ws_addr.clone(),
            enable_register: true,
        })
    }
}
