// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::TcpListener;
use std::process;
use std::sync::Arc;
use std::thread;

use super::handle::ChannelHandle;
use crate::store::fst::StoreFST;
use crate::store::kv::StoreKV;
use crate::{APP_CONF, THREAD_NAME_CHANNEL_CLIENT, THREAD_NAME_CHANNEL_MASTER};

pub struct ChannelListenBuilder;
pub struct ChannelListen;

impl ChannelListenBuilder {
    pub fn new() -> ChannelListen {
        ChannelListen {}
    }
}

impl ChannelListen {
    pub fn run(&self, kv_store: Arc<StoreKV>, fst_store: Arc<StoreFST>) {
        thread::Builder::new()
            .name(THREAD_NAME_CHANNEL_MASTER.to_string())
            .spawn(move || {
                match TcpListener::bind(APP_CONF.channel.inet) {
                    Ok(listener) => {
                        for stream in listener.incoming() {
                            match stream {
                                Ok(stream) => {
                                    let (kv_store_client, fst_store_client) =
                                        (kv_store.clone(), fst_store.clone());

                                    thread::Builder::new()
                                        .name(THREAD_NAME_CHANNEL_CLIENT.to_string())
                                        .spawn(move || {
                                            if let Ok(peer_addr) = stream.peer_addr() {
                                                debug!("channel client connecting: {}", peer_addr);
                                            }

                                            // Create client
                                            ChannelHandle::client(
                                                stream,
                                                kv_store_client,
                                                fst_store_client,
                                            );
                                        })
                                        .ok();
                                }
                                Err(err) => {
                                    warn!("error handling stream: {}", err);
                                }
                            }
                        }

                        info!("listening on tcp://{}", APP_CONF.channel.inet);
                    }
                    Err(err) => {
                        error!("error binding channel listener: {}", err);

                        // Exit Sonic
                        process::exit(1);
                    }
                }
            })
            .ok();
    }
}
