use chain::{IndexedBlock, IndexedBlockHeader};
use message::types;
use p2p::{InboundSyncConnection, InboundSyncConnectionRef, InboundSyncConnectionStateRef};
use types::{LocalNodeRef, PeerIndex, PeersRef, RequestId};
use utils::KnownHashType;

/// Inbound synchronization connection
pub struct InboundConnection {
    /// Index of peer for this connection
    peer_index: PeerIndex,
    /// Peers reference
    peers: PeersRef,
    /// Reference to synchronization node
    node: LocalNodeRef,
}

impl InboundConnection {
    /// Create new inbound connection
    pub fn new(peer_index: PeerIndex, peers: PeersRef, node: LocalNodeRef) -> InboundConnection {
        InboundConnection {
            peer_index: peer_index,
            peers: peers,
            node: node,
        }
    }

    /// Box inbound connection
    pub fn boxed(self) -> InboundSyncConnectionRef {
        Box::new(self)
    }
}

impl InboundSyncConnection for InboundConnection {
    fn sync_state(&self) -> InboundSyncConnectionStateRef {
        self.node.sync_state()
    }

    fn start_sync_session(&self, peer_name: String, version: types::Version) {
        self.node.on_connect(self.peer_index, peer_name, version);
    }

    fn close_session(&self) {
        self.peers.remove(self.peer_index);
        self.node.on_disconnect(self.peer_index);
    }

    fn on_inventory(&self, message: types::Inv) {
        // if inventory is empty - just ignore this message
        if message.inventory.is_empty() {
            return;
        }
        // if inventory length is too big => possible DOS
        if message.inventory.len() > types::INV_MAX_INVENTORY_LEN {
            self.peers.dos(
                self.peer_index,
                &format!("'inv' message contains {} entries", message.inventory.len()),
            );
            return;
        }

        self.node.on_inventory(self.peer_index, message);
    }

    fn on_getdata(&self, message: types::GetData) {
        // if inventory is empty - just ignore this message
        if message.inventory.is_empty() {
            return;
        }
        // if inventory length is too big => possible DOS
        if message.inventory.len() > types::GETDATA_MAX_INVENTORY_LEN {
            self.peers.dos(
                self.peer_index,
                &format!(
                    "'getdata' message contains {} entries",
                    message.inventory.len()
                ),
            );
            return;
        }

        self.node.on_getdata(self.peer_index, message);
    }

    fn on_getblocks(&self, message: types::GetBlocks) {
        self.node.on_getblocks(self.peer_index, message);
    }

    fn on_getheaders(&self, message: types::GetHeaders, id: RequestId) {
        self.node.on_getheaders(self.peer_index, message, id);
    }

    fn on_block(&self, message: types::Block) {
        let block = IndexedBlock::from_raw(message.block);
        self.peers
            .hash_known_as(self.peer_index, block.hash().clone(), KnownHashType::Block);
        self.node.on_block(self.peer_index, block);
    }

    fn on_headers(&self, message: types::Headers) {
        // if headers are empty - just ignore this message
        if message.headers.is_empty() {
            return;
        }
        // if there are too many headers => possible DOS
        if message.headers.len() > types::HEADERS_MAX_HEADERS_LEN {
            self.peers.dos(
                self.peer_index,
                &format!(
                    "'headers' message contains {} headers",
                    message.headers.len()
                ),
            );
            return;
        }

        let headers = message
            .headers
            .into_iter()
            .map(IndexedBlockHeader::from_raw)
            .collect();
        self.node.on_headers(self.peer_index, headers);
    }

    fn on_sendheaders(&self, message: types::SendHeaders) {
        self.node.on_sendheaders(self.peer_index, message);
    }

    fn on_notfound(&self, message: types::NotFound) {
        self.node.on_notfound(self.peer_index, message);
    }
}

#[cfg(test)]
pub mod tests {
    use message::types;
    use p2p::OutboundSyncConnection;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use types::RequestId;

    pub struct DummyOutboundSyncConnection {
        pub messages: Mutex<HashMap<String, usize>>,
    }

    impl DummyOutboundSyncConnection {
        pub fn new() -> Arc<DummyOutboundSyncConnection> {
            Arc::new(DummyOutboundSyncConnection {
                messages: Mutex::new(HashMap::new()),
            })
        }
    }

    impl OutboundSyncConnection for DummyOutboundSyncConnection {
        fn send_inventory(&self, _message: &types::Inv) {
            *self
                .messages
                .lock()
                .entry("inventory".to_owned())
                .or_insert(0) += 1;
        }
        fn send_getdata(&self, _message: &types::GetData) {
            *self
                .messages
                .lock()
                .entry("getdata".to_owned())
                .or_insert(0) += 1;
        }
        fn send_getblocks(&self, _message: &types::GetBlocks) {
            *self
                .messages
                .lock()
                .entry("getblocks".to_owned())
                .or_insert(0) += 1;
        }
        fn send_getheaders(&self, _message: &types::GetHeaders) {
            *self
                .messages
                .lock()
                .entry("getheaders".to_owned())
                .or_insert(0) += 1;
        }
        fn send_block(&self, _message: &types::Block) {
            *self.messages.lock().entry("block".to_owned()).or_insert(0) += 1;
        }
        fn send_headers(&self, _message: &types::Headers) {
            *self
                .messages
                .lock()
                .entry("headers".to_owned())
                .or_insert(0) += 1;
        }
        fn respond_headers(&self, _message: &types::Headers, _id: RequestId) {
            *self
                .messages
                .lock()
                .entry("headers".to_owned())
                .or_insert(0) += 1;
        }
        fn send_sendheaders(&self, _message: &types::SendHeaders) {
            *self
                .messages
                .lock()
                .entry("sendheaders".to_owned())
                .or_insert(0) += 1;
        }
        fn send_notfound(&self, _message: &types::NotFound) {
            *self
                .messages
                .lock()
                .entry("notfound".to_owned())
                .or_insert(0) += 1;
        }
        fn ignored(&self, _id: RequestId) {}
        fn close(&self) {}
    }
}
