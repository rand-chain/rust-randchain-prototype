use chain::Block;
use hash::H256;
use kv::{Key, KeyState, KeyValue, KeyValueDatabase, Operation, Transaction, Value};
use lru_cache::LruCache;
use parking_lot::Mutex;

pub struct CacheDatabase<T>
where
    T: KeyValueDatabase,
{
    db: T,
    block: Mutex<LruCache<H256, KeyState<Block>>>,
}

impl<T> CacheDatabase<T>
where
    T: KeyValueDatabase,
{
    pub fn new(db: T) -> Self {
        CacheDatabase {
            db: db,
            // TODO: reconfig this
            // 144 (blocks per day) * 14 (days) + 100 (arbitrary number)
            block: Mutex::new(LruCache::new(2116)),
        }
    }
}

impl<T> KeyValueDatabase for CacheDatabase<T>
where
    T: KeyValueDatabase,
{
    fn write(&self, tx: Transaction) -> Result<(), String> {
        for op in &tx.operations {
            match *op {
                Operation::Insert(KeyValue::Block(ref hash, ref block)) => {
                    self.block
                        .lock()
                        .insert(hash.clone(), KeyState::Insert(block.clone()));
                }
                Operation::Delete(Key::Block(ref hash)) => {
                    self.block.lock().insert(hash.clone(), KeyState::Delete);
                }
                _ => (),
            }
        }
        self.db.write(tx)
    }

    fn get(&self, key: &Key) -> Result<KeyState<Value>, String> {
        if let Key::Block(ref hash) = *key {
            let mut block = self.block.lock();
            if let Some(state) = block.get_mut(hash) {
                return Ok(state.clone().map(Value::Block));
            }
        }
        self.db.get(key)
    }
}
