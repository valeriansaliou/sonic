// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::TcpListener;
use std::process;
use std::sync::{Arc, RwLock};
use std::thread;

use sonic::store::fst::StoreFSTPool;
use sonic::store::kv::StoreKVPool;

use super::handle::ChannelHandle;
use crate::THREAD_NAME_CHANNEL_CLIENT;

#[derive(Clone)]
pub struct ChannelListenBuilder {
    pub app_conf: Arc<crate::Config>,
    pub kv_pool: StoreKVPool,
    pub fst_pool: StoreFSTPool,
}

pub struct ChannelListen {
    app_conf: Arc<crate::Config>,
    kv_pool: StoreKVPool,
    fst_pool: StoreFSTPool,
}

lazy_static! {
    pub static ref CHANNEL_AVAILABLE: RwLock<bool> = RwLock::new(true);
}

impl ChannelListenBuilder {
    pub fn build(&self) -> ChannelListen {
        ChannelListen {
            app_conf: Arc::clone(&self.app_conf),
            kv_pool: self.kv_pool.clone(),
            fst_pool: self.fst_pool.clone(),
        }
    }
}

impl ChannelListen {
    pub fn run(&self) {
        match TcpListener::bind(self.app_conf.channel.inet) {
            Ok(listener) => {
                tracing::info!("listening on tcp://{}", self.app_conf.channel.inet);

                for stream in listener.incoming() {
                    match stream {
                        Ok(stream) => {
                            let handle = ChannelHandle {
                                app_conf: Arc::clone(&self.app_conf),
                                executor: sonic::Executor {
                                    app_conf: Arc::clone(&self.app_conf.sonic),
                                    kv_pool: self.kv_pool.clone(),
                                    fst_pool: self.fst_pool.clone(),
                                },
                            };

                            thread::Builder::new()
                                .name(THREAD_NAME_CHANNEL_CLIENT.to_string())
                                .spawn(move || {
                                    if let Ok(peer_addr) = stream.peer_addr() {
                                        tracing::debug!("channel client connecting: {}", peer_addr);
                                    }

                                    // Create client
                                    handle.client(stream);
                                })
                                .ok();
                        }
                        Err(err) => {
                            tracing::warn!("error handling stream: {}", err);
                        }
                    }
                }
            }
            Err(err) => {
                tracing::error!("error binding channel listener: {}", err);

                // Exit Sonic
                process::exit(1);
            }
        }
    }

    pub fn teardown() {
        // Channel cannot be used anymore
        *CHANNEL_AVAILABLE.write().unwrap() = false;
    }
}
