use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tandem_board_core::service::Service;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::tools::TandemTools;

/// Where the MCP server listens unless configured otherwise.
pub const DEFAULT_ADDR: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 4317);

/// The URL path of a project's MCP endpoint.
pub fn endpoint_path(project_id: &str) -> String {
    format!("/p/{project_id}/mcp")
}

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("the MCP server only listens on localhost, not {0}")]
    NotLoopback(SocketAddr),
    #[error("cannot listen on {addr}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: io::Error,
    },
}

/// An axum router with one Streamable HTTP endpoint per project, at
/// `/p/<project-id>/mcp`. Every other path is a 404.
///
/// Requests must name a localhost `Host`, which guards against DNS
/// rebinding, and must not come from a browser: any request carrying an
/// `Origin` header is refused, since agents never send one.
pub fn router(
    projects: &BTreeMap<String, Arc<Service>>,
    cancel: &CancellationToken,
) -> axum::Router {
    let mut router = axum::Router::new();
    for (id, service) in projects {
        let service = Arc::clone(service);
        let config = StreamableHttpServerConfig::default()
            .enforce_origin_validation()
            .with_cancellation_token(cancel.child_token());
        let mcp = StreamableHttpService::new(
            move || Ok(TandemTools::new(Arc::clone(&service))),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        router = router.nest_service(&endpoint_path(id), mcp);
    }
    router
}

/// The MCP server, bound and ready to run.
pub struct McpServer {
    listener: TcpListener,
    router: axum::Router,
    cancel: CancellationToken,
}

impl McpServer {
    /// Binds `addr`, which must be a loopback address, and sets up an
    /// endpoint for each project. Cancelling `cancel` stops the server.
    pub async fn bind(
        addr: SocketAddr,
        projects: &BTreeMap<String, Arc<Service>>,
        cancel: CancellationToken,
    ) -> Result<Self, ServeError> {
        if !addr.ip().is_loopback() {
            return Err(ServeError::NotLoopback(addr));
        }
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|source| ServeError::Bind { addr, source })?;
        Ok(Self {
            listener,
            router: router(projects, &cancel),
            cancel,
        })
    }

    /// The bound address, with the real port if `addr` used port 0.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the cancellation token is cancelled.
    pub async fn run(self) -> io::Result<()> {
        let cancel = self.cancel;
        axum::serve(self.listener, self.router)
            .with_graceful_shutdown(async move { cancel.cancelled_owned().await })
            .await
    }
}
