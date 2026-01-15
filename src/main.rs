use crate::core::Config;
use crate::core::Core;
use crate::core::CoreConfig;
use crate::net::{Net, NetConfig};
use anyhow::{anyhow, Result};
use chrono::DateTime;
use chrono::Utc;
use clap::Parser;
use crossbeam::channel;
use log::error;
use log::info;
use tokio::runtime::Runtime;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::{fs::File, io::BufReader};

mod core;
mod data;
mod db;
mod ipc;
mod net;
mod storage;
mod web;
mod ws;

pub(crate) type ServiceDone = Result<DateTime<Utc>>;

#[derive(Parser, Debug)]
pub struct LaunchConfig {
    #[arg(short, long)]
    config: PathBuf,
}

fn main() -> Result<()> {
    pretty_env_logger::init();
    let lc = LaunchConfig::parse();

    let fh = File::open(&lc.config)?;
    let mut buf = BufReader::new(fh);

    let c: Config = serde_json::from_reader(buf)?;
    let (s_stop, r_stop) = tokio::sync::watch::channel(Utc::now());
    let (web_chans, ws_chans, core_chans) = Net::make_chans(r_stop.clone());
    let net_config = NetConfig::try_from(&c)?;
    let rt = Runtime::new().unwrap();
    let hd = rt.handle();
    let mut js = JoinSet::new();
    let mut term = 0;
    let cc = CoreConfig::from(&c);
    term += Net::serve(&hd, &mut js, net_config, web_chans, ws_chans, r_stop.clone());
    term += Core::serve(&hd, &mut js, cc, core_chans);
    let ctrlc: JoinHandle<Result<()>> = hd.spawn(async move {
        signal::ctrl_c().await?;
        info!("Gracefully shutting down");
        s_stop.send(Utc::now())?;
        Ok(())
    });
    hd.block_on(async move {
        while let Some(ev_res) = js.join_next().await {
            match ev_res {
                Ok(Ok(shutdown_time)) => {
                    info!("Service shut down at {}", shutdown_time);
                    term -= 1;
                    if term == 0 {
                        return Ok(());
                    }
                },
                Ok(Err(e)) => {
                    // service error
                    error!("Service error: {}", e);
                    js.shutdown().await;
                    break;
                },
                Err(e) => {
                    error!("Join error: {}", e);
                    js.shutdown().await;
                    break;
                }
            }
        }
        Err(anyhow!("unclean shutdown"))
    })
}
