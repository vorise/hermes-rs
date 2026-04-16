//! Hermes ACP Crate
//!
//! Agent Communication Protocol server for IDE integration.

pub mod server;
pub mod protocol;

pub use server::{AcpServer, AcpConfig, AcpSession, AcpError};
pub use protocol::{
    AcpRequest, AcpResponse, SelectionContext, SelectionType,
    GitCommand, TerminalCommand, SessionStatus, ToolCallInfo,
    parse_request, serialize_response,
};