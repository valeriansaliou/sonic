// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(clippy::type_complexity)]

//! Documentation in progress. See examples.
//!
#![cfg_attr(feature = "ingest", doc = "- Ingest mode: see [`ingest`]")]
#![cfg_attr(feature = "control", doc = "- Control mode: see [`control`]")]
#![cfg_attr(feature = "search", doc = "- Search mode: see [`search`]")]

#[cfg(not(any(feature = "ingest", feature = "control", feature = "search")))]
compile_error!(
    "This library is pointless without the \"ingest\", \"control\" or \"search\" feature flag enabled."
);

#[cfg(feature = "control")]
pub mod control;
mod events;
#[cfg(feature = "ingest")]
pub mod ingest;
pub mod options;
#[cfg(feature = "search")]
pub mod search;
pub mod transport;
mod util;
#[macro_use]
mod logging;
mod channel;
mod connection;
mod multiplexer;

pub(crate) use crate::channel::Command;
pub use crate::events::{ChannelInfo, ServerInfo};
pub use crate::multiplexer::SonicMultiplexer;

/// Number of commands which can be queued between two cycles of the event loop.
///
/// The difference in allocated memory is quite small so there’s no reason to
/// keep this very low. We might make this configurable someday.
const COMMAND_QUEUE_SIZE: usize = 64;

/// Number of [`mio`] events that can be processed in a single event loop cycle.
/// Real usage should be less than or equal to 3 times the number of Sonic
/// channels you’ve attached to a single [`SonicMultiplexer`].
///
/// The difference in allocated memory is quite small so there’s no reason to
/// keep this very low. We might make this configurable someday.
const MIO_EVENTS_CAPACITY: usize = 3 * 16;

/// Capacity of the buffer where Sonic responses are written to before being
/// processed. Sonic server responses are usually quite short so no need to
/// make it huge.
///
/// We chose 2 * 4KiB (common memory page size) as the default, which should
/// be good for most use cases. We might make this configurable someday.
const TCP_READ_BUFFER_CAPACITY: usize = 2 * 4096;

/// Capacity of the buffer where Sonic commands are written to before being
/// flushed to the TCP stream. Ideally, it should be larger than your Sonic
/// server’s buffer size (which is often 20 000).
///
/// We chose 16 * 4KiB (common memory page size) as the default, which should
/// be good for most use cases. We might make this configurable someday.
const TCP_WRITE_BUFFER_CAPACITY: usize = 65_536;

/// Safety timeout when sending events to channels.
///
/// This timeout can be reached if channel capacities are too low.
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Safety timeout when reading events from channels.
///
/// This timeout has to take into account TCP packets travel time, parsing and
/// processing, hence why it’s so high. It should not be reached, but it’s there
/// as a safety precaution.
const RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
