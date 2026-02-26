use std::sync::Arc;

use tokio::sync::broadcast;
use txviz_chain::types::ChainIdentity;
use txviz_core::model::BlockUpdateEvent;
use txviz_storage::backend::StorageBackend;

pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub chain_identity: ChainIdentity,
    pub live_tx: broadcast::Sender<BlockUpdateEvent>,
}
