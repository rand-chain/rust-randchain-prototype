use bytes::Bytes;
use chain::Block;
use hash::H256;
use kv::{Key, KeyState, KeyValue, KeyValueDatabase, Operation, Transaction, Value};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::mem::replace;
use std::sync::Arc;

#[derive(Default, Debug)]
struct InnerDatabase {
    meta: HashMap<&'static str, KeyState<Bytes>>,
    block_hash: HashMap<u32, KeyState<H256>>,
    block: HashMap<H256, KeyState<Block>>,
    block_number: HashMap<H256, KeyState<u32>>,
    configuration: HashMap<&'static str, KeyState<Bytes>>,
}

#[derive(Default, Debug)]
pub struct MemoryDatabase {
    db: RwLock<InnerDatabase>,
}

impl MemoryDatabase {
    pub fn drain_transaction(&self) -> Transaction {
        let mut db = self.db.write();
        let meta = replace(&mut db.meta, HashMap::default())
            .into_iter()
            .flat_map(|(key, state)| state.into_operation(key, KeyValue::Meta, Key::Meta));

        let block_hash = replace(&mut db.block_hash, HashMap::default())
            .into_iter()
            .flat_map(|(key, state)| {
                state.into_operation(key, KeyValue::BlockHash, Key::BlockHash)
            });

        let block = replace(&mut db.block, HashMap::default())
            .into_iter()
            .flat_map(|(key, state)| state.into_operation(key, KeyValue::Block, Key::Block));

        let block_number = replace(&mut db.block_number, HashMap::default())
            .into_iter()
            .flat_map(|(key, state)| {
                state.into_operation(key, KeyValue::BlockNumber, Key::BlockNumber)
            });

        let configuration = replace(&mut db.configuration, HashMap::default())
            .into_iter()
            .flat_map(|(key, state)| {
                state.into_operation(key, KeyValue::Configuration, Key::Configuration)
            });

        Transaction {
            operations: meta
                .chain(block_hash)
                .chain(block)
                .chain(block_number)
                .chain(configuration)
                .collect(),
        }
    }
}

impl KeyValueDatabase for MemoryDatabase {
    fn write(&self, tx: Transaction) -> Result<(), String> {
        let mut db = self.db.write();
        for op in tx.operations.into_iter() {
            match op {
                Operation::Insert(insert) => match insert {
                    KeyValue::Meta(key, value) => {
                        db.meta.insert(key, KeyState::Insert(value));
                    }
                    KeyValue::BlockHash(key, value) => {
                        db.block_hash.insert(key, KeyState::Insert(value));
                    }
                    KeyValue::Block(key, value) => {
                        db.block.insert(key, KeyState::Insert(value));
                    }
                    KeyValue::BlockNumber(key, value) => {
                        db.block_number.insert(key, KeyState::Insert(value));
                    }
                    KeyValue::Configuration(key, value) => {
                        db.configuration.insert(key, KeyState::Insert(value));
                    }
                },
                Operation::Delete(delete) => match delete {
                    Key::Meta(key) => {
                        db.meta.insert(key, KeyState::Delete);
                    }
                    Key::BlockHash(key) => {
                        db.block_hash.insert(key, KeyState::Delete);
                    }
                    Key::Block(key) => {
                        db.block.insert(key, KeyState::Delete);
                    }
                    Key::BlockNumber(key) => {
                        db.block_number.insert(key, KeyState::Delete);
                    }
                    Key::Configuration(key) => {
                        db.configuration.insert(key, KeyState::Delete);
                    }
                },
            }
        }
        Ok(())
    }

    fn get(&self, key: &Key) -> Result<KeyState<Value>, String> {
        let db = self.db.read();
        let result = match *key {
            Key::Meta(ref key) => db
                .meta
                .get(key)
                .cloned()
                .unwrap_or_default()
                .map(Value::Meta),
            Key::BlockHash(ref key) => db
                .block_hash
                .get(key)
                .cloned()
                .unwrap_or_default()
                .map(Value::BlockHash),
            Key::Block(ref key) => db
                .block
                .get(key)
                .cloned()
                .unwrap_or_default()
                .map(Value::Block),
            Key::BlockNumber(ref key) => db
                .block_number
                .get(key)
                .cloned()
                .unwrap_or_default()
                .map(Value::BlockNumber),
            Key::Configuration(ref key) => db
                .configuration
                .get(key)
                .cloned()
                .unwrap_or_default()
                .map(Value::Configuration),
        };

        Ok(result)
    }
}

#[derive(Debug)]
pub struct SharedMemoryDatabase {
    db: Arc<MemoryDatabase>,
}

impl Default for SharedMemoryDatabase {
    fn default() -> Self {
        SharedMemoryDatabase { db: Arc::default() }
    }
}

impl Clone for SharedMemoryDatabase {
    fn clone(&self) -> Self {
        SharedMemoryDatabase {
            db: self.db.clone(),
        }
    }
}

impl KeyValueDatabase for SharedMemoryDatabase {
    fn write(&self, tx: Transaction) -> Result<(), String> {
        self.db.write(tx)
    }

    fn get(&self, key: &Key) -> Result<KeyState<Value>, String> {
        self.db.get(key)
    }
}
