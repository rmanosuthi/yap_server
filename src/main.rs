#![feature(proc_macro_hygiene, decl_macro)]
#![feature(async_closure)]
#![feature(min_const_generics)]

mod common;
mod core;
mod data;
mod db;
mod errors;
mod msg;
mod net;
mod storage;
mod web;
mod ws;

#[macro_use]
extern crate structopt;

/// Internal symbols for easier importing in individual files.
pub mod symbols {
    pub use crate::common::*;
    pub use crate::core::*;
    pub use crate::data::*;
    pub use crate::db::*;
    pub use crate::errors::*;
    pub use crate::msg::*;
    pub use crate::net::*;
    pub use crate::storage::*;
    pub use crate::web::*;
    pub use crate::ws::*;
}

/// Imports this crate uses.
pub mod imports {
    pub use async_trait::async_trait;
    pub use chrono::{NaiveDateTime, DateTime, Utc};
    pub use crossbeam::sync::ShardedLock;
    pub use futures::{Stream, StreamExt, SinkExt, TryFutureExt};
    pub use hashbrown::{HashMap, HashSet};
    pub use log::{debug, error, info, warn};
    pub use rand::{rngs::ThreadRng, Rng};
    pub use serde::{Deserialize, Serialize, de::DeserializeOwned};
    pub use std::collections::VecDeque;
    pub use std::error::Error;
    pub use std::fmt::Display;
    pub use std::hash::Hash;
    pub use std::fs::File;
    pub use std::io::{BufReader};
    pub use std::path::{Path, PathBuf};
    pub use std::pin::Pin;
    pub use std::sync::Arc;
    pub use std::time::{Duration, Instant, SystemTime};
    pub use tokio::{net::TcpStream, runtime::Runtime};
    pub use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
        sync::{
            mpsc,
            mpsc::{Receiver, Sender},
            RwLock,
        },
    };
    pub use tokio_tungstenite::WebSocketStream;
    pub use uuid::Uuid;
    pub use warp::{
        http::{Response, StatusCode},
        reject::Reject,
        Filter, Reply,
    };
    pub use mysql::prelude::*;
    pub use tui::backend::CrosstermBackend;
    pub use tui::layout::{Constraint, Direction, Layout};
    pub use tui::widgets::{Block, Borders, Widget};
    pub use tui::Terminal;
    pub use structopt::StructOpt;
}

use crate::imports::*;
use crate::symbols::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(StructOpt, Debug)]
pub struct LaunchConfig {
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    json_path: PathBuf
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let lc = LaunchConfig::from_args();

    let fh = File::open(&lc.json_path)?;
    let mut buf = BufReader::new(fh);

    let c = serde_json::from_reader(buf)?;
    let (s_stop, r_stop) = tokio::sync::watch::channel(chrono::Utc::now());
    let (web_chans, ws_chans, core_chans) = Net::make_chans(r_stop.clone());
    let net_config = NetConfig::from(&c);
    match Net::build(net_config, web_chans, ws_chans, r_stop.clone()) {
        Ok(mut net) => {
            let running = Arc::new(AtomicBool::new(true));
            let (ctrlc_s, ctrlc_r) = crossbeam::channel::bounded(1);
            Core::run(
                CoreConfig::from(&c),
                core_chans,
            );

            ctrlc::set_handler(move || {
                ctrlc_s.send(());
            })
            .expect("Error setting Ctrl-C handler");

            println!("Waiting for Ctrl-C...");
            ctrlc_r.recv();

            s_stop.broadcast(Utc::now());
            println!("Got it! Exiting...");
        }
        Err(e) => {}
    }
    Ok(())
}
