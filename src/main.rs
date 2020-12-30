#![feature(proc_macro_hygiene, decl_macro)]
#![feature(async_closure)]

mod common;
mod core;
mod db;
mod errors;
mod msg;
mod net;
mod storage;
mod web;
mod ws;

pub mod symbols {
    pub use crate::common::*;
    pub use crate::core::*;
    pub use crate::db::*;
    pub use crate::errors::*;
    pub use crate::msg::*;
    pub use crate::net::*;
    pub use crate::storage::*;
    pub use crate::web::*;
    pub use crate::ws::*;
}

pub mod imports {
    pub use async_trait::async_trait;
    pub use chrono::{DateTime, Utc};
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
    pub use std::path::{Path, PathBuf};
    pub use std::pin::Pin;
    pub use std::sync::Arc;
    pub use std::time::Duration;
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
}

use crate::imports::*;
use crate::symbols::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    env_logger::init();
    let (s_stop, r_stop) = tokio::sync::watch::channel(chrono::Utc::now());
    let (web_chans, ws_chans, core_chans) = Net::make_chans(r_stop.clone());
    let net_config = NetConfig {
        api_addr: "127.0.0.1:8080".to_string(),
        ws_addr: "127.0.0.1:9999".to_string(),
        enable_register: true,
    };
    match Net::build(net_config, web_chans, ws_chans, r_stop.clone()) {
        Ok(mut net) => {
            let running = Arc::new(AtomicBool::new(true));
            let (ctrlc_s, ctrlc_r) = crossbeam::channel::bounded(1);

            // let's not hardcode user-passwords
            // TODO change to actual values
            Core::run(
                CoreConfig {
                    mysql_addr: "127.0.0.1".to_owned(),
                    mysql_port: 3306,
                    mysql_user: "".to_owned(),
                    mysql_pass: "".to_owned()
                },
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
}
