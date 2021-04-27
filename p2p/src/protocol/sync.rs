use bytes::Bytes;
use message::{deserialize_payload, types, Command, Error, Payload, Services};
use net::PeerContext;
use protocol::Protocol;
use std::sync::Arc;

pub type InboundSyncConnectionRef = Box<dyn InboundSyncConnection>;
pub type OutboundSyncConnectionRef = Arc<dyn OutboundSyncConnection>;
pub type LocalSyncNodeRef = Box<dyn LocalSyncNode>;
pub type InboundSyncConnectionStateRef = Arc<dyn InboundSyncConnectionState>;

pub trait LocalSyncNode: Send + Sync {
    fn create_sync_session(
        &self,
        height: i32,
        services: Services,
        outbound: OutboundSyncConnectionRef,
    ) -> InboundSyncConnectionRef;
}

pub trait InboundSyncConnectionState: Send + Sync {
    fn synchronizing(&self) -> bool;
}

pub trait InboundSyncConnection: Send + Sync {
    fn sync_state(&self) -> InboundSyncConnectionStateRef;
    fn start_sync_session(&self, peer_name: String, version: types::Version);
    fn close_session(&self);
    fn on_inventory(&self, message: types::Inv);
    fn on_getdata(&self, message: types::GetData);
    fn on_getblocks(&self, message: types::GetBlocks);
    fn on_getheaders(&self, message: types::GetHeaders, id: u32);
    fn on_block(&self, message: types::Block);
    fn on_headers(&self, message: types::Headers);
    fn on_sendheaders(&self, message: types::SendHeaders);
    fn on_notfound(&self, message: types::NotFound);
}

pub trait OutboundSyncConnection: Send + Sync {
    fn send_inventory(&self, message: &types::Inv);
    fn send_getdata(&self, message: &types::GetData);
    fn send_getblocks(&self, message: &types::GetBlocks);
    fn send_getheaders(&self, message: &types::GetHeaders);
    fn send_block(&self, message: &types::Block);
    fn send_headers(&self, message: &types::Headers);
    fn respond_headers(&self, message: &types::Headers, id: u32);
    fn send_sendheaders(&self, message: &types::SendHeaders);
    fn send_notfound(&self, message: &types::NotFound);
    fn ignored(&self, id: u32);
    fn close(&self);
}

struct OutboundSync {
    context: Arc<PeerContext>,
}

impl OutboundSync {
    pub fn new(context: Arc<PeerContext>) -> OutboundSync {
        OutboundSync { context: context }
    }
}

impl OutboundSyncConnection for OutboundSync {
    fn send_inventory(&self, message: &types::Inv) {
        self.context.send_request(message);
    }

    fn send_getdata(&self, message: &types::GetData) {
        self.context.send_request(message);
    }

    fn send_getblocks(&self, message: &types::GetBlocks) {
        self.context.send_request(message);
    }

    fn send_getheaders(&self, message: &types::GetHeaders) {
        self.context.send_request(message);
    }

    fn send_block(&self, message: &types::Block) {
        self.context.send_request(message);
    }

    fn send_headers(&self, message: &types::Headers) {
        self.context.send_request(message);
    }

    fn respond_headers(&self, message: &types::Headers, id: u32) {
        self.context.send_response(message, id, true);
    }

    fn send_sendheaders(&self, message: &types::SendHeaders) {
        self.context.send_request(message);
    }

    fn send_notfound(&self, message: &types::NotFound) {
        self.context.send_request(message);
    }

    fn ignored(&self, id: u32) {
        self.context.ignore_response(id);
    }

    fn close(&self) {
        self.context
            .global()
            .penalize_node(&self.context.info().address);
        self.context.close()
    }
}

pub struct SyncProtocol {
    inbound_connection: InboundSyncConnectionRef,
    context: Arc<PeerContext>,
    state: InboundSyncConnectionStateRef,
}

impl SyncProtocol {
    pub fn new(context: Arc<PeerContext>) -> Self {
        let outbound_connection = Arc::new(OutboundSync::new(context.clone()));
        let inbound_connection = context.global().create_sync_session(
            0,
            context.info().version_message.services(),
            outbound_connection,
        );
        let state = inbound_connection.sync_state();
        SyncProtocol {
            inbound_connection: inbound_connection,
            context: context,
            state: state,
        }
    }
}

impl Protocol for SyncProtocol {
    fn initialize(&mut self) {
        let info = self.context.info();
        self.inbound_connection.start_sync_session(
            format!("{}/{}", info.address, info.user_agent),
            info.version_message.clone(),
        );
    }

    fn on_message(&mut self, command: &Command, payload: &Bytes) -> Result<(), Error> {
        let version = self.context.info().version;
        if command == &types::Inv::command() {
            // we are synchronizing => we ask only for blocks with known headers
            // => there are no useful blocks hashes for us
            // we are synchronizing
            // => we ignore all transactions until it is completed => there are no useful transactions hashes for us
            if self.state.synchronizing() {
                return Ok(());
            }

            let message: types::Inv = deserialize_payload(payload, version)?;
            self.inbound_connection.on_inventory(message);
        } else if command == &types::GetData::command() {
            if self.state.synchronizing() {
                return Ok(());
            }

            let message: types::GetData = deserialize_payload(payload, version)?;
            self.inbound_connection.on_getdata(message);
        } else if command == &types::GetBlocks::command() {
            if self.state.synchronizing() {
                return Ok(());
            }

            let message: types::GetBlocks = deserialize_payload(payload, version)?;
            self.inbound_connection.on_getblocks(message);
        } else if command == &types::GetHeaders::command() {
            if self.state.synchronizing() {
                return Ok(());
            }

            let message: types::GetHeaders = deserialize_payload(payload, version)?;
            let id = self.context.declare_response();
            trace!(
                "declared response {} for request: {}",
                id,
                types::GetHeaders::command()
            );
            self.inbound_connection.on_getheaders(message, id);
        } else if command == &types::Block::command() {
            let message: types::Block = deserialize_payload(payload, version)?;
            self.inbound_connection.on_block(message);
        } else if command == &types::Headers::command() {
            let message: types::Headers = deserialize_payload(payload, version)?;
            self.inbound_connection.on_headers(message);
        } else if command == &types::SendHeaders::command() {
            let message: types::SendHeaders = deserialize_payload(payload, version)?;
            self.inbound_connection.on_sendheaders(message);
        } else if command == &types::NotFound::command() {
            let message: types::NotFound = deserialize_payload(payload, version)?;
            self.inbound_connection.on_notfound(message);
        }
        Ok(())
    }

    fn on_close(&mut self) {
        self.inbound_connection.close_session()
    }
}
