// BLACKER
// Copyright (C) 2026 Juan José Caballero Rey
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation version 3 of the License.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::{routes, state::AppState};
use std::time::Duration;

use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use tokio::time::timeout;
use tracing::{info, warn};

const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub async fn start_http_server(state: AppState) {
    let app: Router = Router::new()
        .nest("/api/market", routes::router())
        .with_state(state)
        .layer(middleware::from_fn(enforce_request_timeout));

    let listener: tokio::net::TcpListener =
        tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();

    info!("Master listening in :3001");

    axum::serve(listener, app).await.unwrap();
}

async fn enforce_request_timeout(request: Request, next: Next) -> Response {
    match timeout(HTTP_REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            warn!(
                timeout_seconds = HTTP_REQUEST_TIMEOUT.as_secs(),
                "HTTP request timed out"
            );
            (StatusCode::GATEWAY_TIMEOUT, "Request timed out").into_response()
        }
    }
}
