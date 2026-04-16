// Hermes messaging platform gateway

pub mod base;
pub mod config;
pub mod cron;
pub mod formatting;
pub mod platforms;
pub mod runner;
pub mod session;

pub use base::{IncomingMessage, MessageFormat, PlatformAdapter, PlatformConfig, StreamConsumer};
pub use config::GatewayConfig;
pub use cron::{CronJob, CronScheduler, CronExpr, DeliveryTarget, JobExecution, SchedulerEvent};
pub use cron::{create_job_from_natural, execute_job, parse_natural};
pub use formatting::{format_message, split_message, strip_markdown, truncate};
pub use runner::{BufferingConsumer, GatewayRunner, GatewayStatus};
pub use session::GatewaySession;
pub use session::GatewaySessionStore;
