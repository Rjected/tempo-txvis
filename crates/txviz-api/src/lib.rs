pub mod routes;
pub mod sse;
pub mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use state::AppState;

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/chain", axum::routing::get(routes::chain_handler))
        .route("/api/block/{number}", axum::routing::get(routes::get_block_handler))
        .route(
            "/api/block/hash/{hash}",
            axum::routing::get(routes::get_block_by_hash_handler),
        )
        .route("/api/blocks", axum::routing::get(routes::list_blocks_handler))
        .route("/api/live", axum::routing::get(sse::live_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
