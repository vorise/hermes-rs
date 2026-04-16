//! Hermes Gateway Crate
//!
//! Messaging platform gateway and adapters.
//! Routes messages from multiple platforms to the Hermes agent.

pub mod runner;
pub mod session;
pub mod stream_consumer;
pub mod platforms;
pub mod pairing;
pub mod formatter;
pub mod event;
pub mod cron;

pub use runner::GatewayRunner;
pub use session::{GatewaySessionStore, GatewaySession};
pub use stream_consumer::{StreamConsumer, GatewayStreamConsumer};
pub use platforms::{PlatformAdapter, PlatformRegistry};
pub use pairing::PairingManager;
pub use formatter::{MessageFormatter, FormatType};
pub use event::{GatewayEvent, GatewayMessage};
pub use cron::{CronScheduler, CronJob, DeliveryTarget, CronSchedule};