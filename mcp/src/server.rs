use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
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

type Mcp = StreamableHttpService<TandemTools, LocalSessionManager>;

/// One project's endpoint, and the token that ends its sessions.
struct Endpoint {
    mcp: Mcp,
    cancel: CancellationToken,
}

/// The projects the server has endpoints for. Projects can be added and
/// removed while the server runs; clones share the same list.
#[derive(Clone)]
pub struct Endpoints {
    projects: Arc<RwLock<BTreeMap<String, Endpoint>>>,
    cancel: CancellationToken,
}

impl Endpoints {
    /// No endpoints yet. Cancelling `cancel`, as stopping the server does,
    /// ends every endpoint's sessions.
    pub fn new(cancel: &CancellationToken) -> Self {
        Self {
            projects: Arc::default(),
            cancel: cancel.clone(),
        }
    }

    /// Serves `service` at the project's endpoint, replacing any endpoint
    /// it already had.
    pub fn insert(&self, project_id: &str, service: Arc<Service>) {
        let cancel = self.cancel.child_token();
        let config = StreamableHttpServerConfig::default()
            .enforce_origin_validation()
            .with_cancellation_token(cancel.clone());
        let mcp = StreamableHttpService::new(
            move || Ok(TandemTools::new(Arc::clone(&service))),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let old = self
            .write()
            .insert(project_id.to_owned(), Endpoint { mcp, cancel });
        if let Some(old) = old {
            old.cancel.cancel();
        }
    }

    /// Removes the project's endpoint and ends its sessions. Returns
    /// `false` if it had none.
    pub fn remove(&self, project_id: &str) -> bool {
        match self.write().remove(project_id) {
            Some(old) => {
                old.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// The ids of the projects with an endpoint, sorted.
    pub fn project_ids(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    fn get(&self, project_id: &str) -> Option<Mcp> {
        self.read()
            .get(project_id)
            .map(|endpoint| endpoint.mcp.clone())
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, Endpoint>> {
        self.projects.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, Endpoint>> {
        self.projects
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// An axum router with one Streamable HTTP endpoint per project in
/// `endpoints`, at `/p/<project-id>/mcp`. Every other path is a 404.
///
/// Requests must name a localhost `Host`, which guards against DNS
/// rebinding, and must not come from a browser: any request carrying an
/// `Origin` header is refused, since agents never send one.
pub fn router(endpoints: Endpoints) -> axum::Router {
    axum::Router::new()
        .route("/p/{project}/mcp", axum::routing::any(serve))
        .with_state(endpoints)
}

async fn serve(
    State(endpoints): State<Endpoints>,
    Path(project): Path<String>,
    request: Request,
) -> Response {
    match endpoints.get(&project) {
        Some(mcp) => mcp.handle(request).await.map(Body::new),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The MCP server, bound and ready to run.
pub struct McpServer {
    listener: TcpListener,
    router: axum::Router,
    cancel: CancellationToken,
}

impl McpServer {
    /// Binds `addr`, which must be a loopback address, to serve the
    /// projects in `endpoints`. Cancelling `cancel` stops the server.
    pub async fn bind(
        addr: SocketAddr,
        endpoints: Endpoints,
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
            router: router(endpoints),
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
