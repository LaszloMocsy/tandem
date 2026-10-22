//! The MCP server inside the app: started on the port from the settings,
//! and moved to another port when you change it. Moving binds the new port
//! first, so a port that is taken leaves the server where it was.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tandem_board_mcp::{Endpoints, McpServer, ServeError};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, describe, report};

/// The running server, if any, and why it isn't running otherwise.
pub struct Mcp {
    endpoints: Endpoints,
    /// Cancelled when the app exits, which stops whichever server runs.
    cancel: CancellationToken,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    /// Why the server failed to start, while none runs.
    error: Option<String>,
}

struct Running {
    addr: SocketAddr,
    /// Stops this server alone.
    cancel: CancellationToken,
}

impl Mcp {
    /// No server yet; [`start`](Self::start) starts one that serves
    /// `endpoints`.
    pub fn new(endpoints: Endpoints, cancel: CancellationToken) -> Self {
        Self {
            endpoints,
            cancel,
            state: Mutex::default(),
        }
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// Serves on `port` on localhost. If a server already runs on another
    /// port, it is stopped once the new one listens, and its sessions end;
    /// if the new port can't be bound, the old server keeps running.
    /// Port 0 picks a free port, which only tests want.
    pub fn start(&self, port: u16) -> Result<SocketAddr, Error> {
        let mut state = self.lock();
        if let Some(running) = &state.running
            && running.addr.port() == port
        {
            return Ok(running.addr);
        }
        let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
        let cancel = self.cancel.child_token();
        let bound = tauri::async_runtime::block_on(McpServer::bind(
            addr,
            self.endpoints.clone(),
            cancel.clone(),
        ))
        .and_then(|server| {
            let addr = server
                .local_addr()
                .map_err(|source| ServeError::Bind { addr, source })?;
            Ok((server, addr))
        });
        let (server, addr) = match bound {
            Ok(bound) => bound,
            Err(e) => {
                let error = bind_error(port, &e);
                if state.running.is_none() {
                    state.error = Some(error.message.clone());
                }
                return Err(error);
            }
        };
        tauri::async_runtime::spawn(async move {
            if let Err(e) = server.run().await {
                report("MCP server stopped", &e);
            }
        });
        eprintln!("tandem: MCP server at http://{addr}");
        let old = state.running.replace(Running { addr, cancel });
        state.error = None;
        drop(state);
        if let Some(old) = old {
            old.cancel.cancel();
            self.endpoints.end_sessions();
        }
        Ok(addr)
    }

    /// Where the server listens, while it runs.
    pub fn addr(&self) -> Option<SocketAddr> {
        self.lock().running.as_ref().map(|running| running.addr)
    }

    /// Why the server isn't running, when it isn't.
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn bind_error(port: u16, error: &ServeError) -> Error {
    match error {
        ServeError::Bind { source, .. } if source.kind() == io::ErrorKind::AddrInUse => Error::new(
            "port_in_use",
            format!("port {port} is in use, maybe by another copy of Tandem; pick another"),
        ),
        _ => Error::new("mcp_failed", describe(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn mcp() -> (Mcp, CancellationToken) {
        let cancel = CancellationToken::new();
        (Mcp::new(Endpoints::new(&cancel), cancel.clone()), cancel)
    }

    fn listening(addr: SocketAddr) -> bool {
        std::net::TcpStream::connect(addr).is_ok()
    }

    #[test]
    fn moves_to_a_new_port_and_frees_the_old_one() {
        let (mcp, cancel) = mcp();
        assert_eq!(mcp.addr(), None);
        let first = mcp.start(0).unwrap();
        assert_eq!(mcp.addr(), Some(first));
        assert!(listening(first));
        // The same port again changes nothing.
        assert_eq!(mcp.start(first.port()).unwrap(), first);

        let second = mcp.start(0).unwrap();
        assert_ne!(second, first);
        assert_eq!(mcp.addr(), Some(second));
        assert!(listening(second));
        // The old port is free again, so it could be chosen once more.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while TcpListener::bind(first).is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "{first} is still taken"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        cancel.cancel();
    }

    #[test]
    fn a_taken_port_keeps_the_server_where_it_was() {
        let (mcp, cancel) = mcp();
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();

        // Nothing runs yet, so the failure is kept to show.
        let error = mcp.start(port).unwrap_err();
        assert_eq!(error.code, "port_in_use");
        assert!(
            error.message.contains(&port.to_string()),
            "{}",
            error.message
        );
        assert_eq!(mcp.addr(), None);
        assert_eq!(mcp.error(), Some(error.message));

        let running = mcp.start(0).unwrap();
        assert_eq!(mcp.error(), None);
        assert_eq!(mcp.start(port).unwrap_err().code, "port_in_use");
        assert_eq!(mcp.addr(), Some(running));
        assert_eq!(mcp.error(), None);
        assert!(listening(running));
        cancel.cancel();
    }
}
