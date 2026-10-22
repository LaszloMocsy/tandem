//! The MCP server for Tandem: the spec's tools over Streamable HTTP, one
//! endpoint per project at `http://127.0.0.1:4317/p/<project-id>/mcp`, on
//! the port set in the app's settings; projects can be added and removed
//! while it runs.
//! Every tool calls the project's [`Service`](tandem_board_core::service::Service),
//! the same entry point the board uses. Has no Tauri dependency, so it can
//! later run headless.

mod server;
mod tools;

pub use server::{DEFAULT_ADDR, Endpoints, McpServer, ServeError, endpoint_path, router};
pub use tools::TandemTools;
