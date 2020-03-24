use std::collections::BTreeMap;
#[cfg(feature = "iterator")]
use std::ops::RangeBounds;

use crate::errors::Result;
use crate::traits::{Api, Extern, ReadonlyStorage, Storage};

#[derive(Default)]
pub struct MemoryStorage {
    data: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        MemoryStorage::default()
    }
}

impl ReadonlyStorage for MemoryStorage {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.get(key).cloned()
    }

    #[cfg(feature = "iterator")]
    /// range allows iteration over a set of keys, either forwards or backwards
    /// uses standard rust range notation, and eg db.range(b"foo"..b"bar") also works reverse
    fn range<R: RangeBounds<Vec<u8>>>(
        &self,
        bounds: R,
    ) -> Box<dyn DoubleEndedIterator<Item = (Vec<u8>, Vec<u8>)>> {
        let iter = self.data.range(bounds);
        // We brute force this a bit to deal with lifetimes.... should do this lazy
        let res: Vec<_> = iter.map(|(k, v)| (k.clone(), v.clone())).collect();
        Box::new(res.into_iter())
    }
}

impl Storage for MemoryStorage {
    fn set(&mut self, key: &[u8], value: &[u8]) {
        self.data.insert(key.to_vec(), value.to_vec());
    }
}

pub struct StorageTransaction<'a, S: ReadonlyStorage> {
    /// read-only access to backing storage
    storage: &'a S,
    /// these are local changes not flushed to backing storage
    local_state: MemoryStorage,
    /// a log of local changes not yet flushed to backing storage
    rep_log: RepLog,
}

pub struct RepLog {
    /// this is a list of changes to be written to backing storage upon commit
    ops_log: Vec<Op>,
}

impl RepLog {
    fn new() -> Self {
        RepLog { ops_log: vec![] }
    }

    /// appends an op to the list of changes to be applied upon commit
    fn append(&mut self, op: Op) {
        self.ops_log.push(op);
    }

    /// applies the stored list of `Op`s to the provided `Storage`
    pub fn commit<S: Storage>(self, storage: &mut S) {
        for op in self.ops_log {
            op.apply(storage);
        }
    }
}

enum Op {
    /// represents the `Set` operation for setting a key-value pair in storage
    Set { key: Vec<u8>, value: Vec<u8> },
}

impl Op {
    /// applies this `Op` to the provided storage
    pub fn apply<S: Storage>(&self, storage: &mut S) {
        match self {
            Op::Set { key, value } => storage.set(&key, &value),
        }
    }
}

impl<'a, S: ReadonlyStorage> StorageTransaction<'a, S> {
    pub fn new(storage: &'a S) -> Self {
        StorageTransaction {
            storage,
            local_state: MemoryStorage::new(),
            rep_log: RepLog::new(),
        }
    }

    /// prepares this transaction to be committed to storage
    pub fn prepare(self) -> RepLog {
        self.rep_log
    }

    /// rollback will consume the checkpoint and drop all changes (no really needed, going out of scope does the same, but nice for clarity)
    pub fn rollback(self) {}
}

impl<'a, S: ReadonlyStorage> ReadonlyStorage for StorageTransaction<'a, S> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.local_state.get(key) {
            Some(val) => Some(val),
            None => self.storage.get(key),
        }
    }

    #[cfg(feature = "iterator")]
    /// range allows iteration over a set of keys, either forwards or backwards
    /// uses standard rust range notation, and eg db.range(b"foo"..b"bar") also works reverse
    fn range<R: RangeBounds<Vec<u8>>>(
        &self,
        bounds: R,
    ) -> Box<dyn DoubleEndedIterator<Item = (Vec<u8>, Vec<u8>)>> {
        // TODO: also combine with underlying storage
        self.local_state.range(bounds)
    }
}

impl<'a, S: ReadonlyStorage> Storage for StorageTransaction<'a, S> {
    fn set(&mut self, key: &[u8], value: &[u8]) {
        let op = Op::Set {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        op.apply(&mut self.local_state);
        self.rep_log.append(op);
    }
}

pub fn transactional<S: Storage, T>(
    storage: &mut S,
    tx: &dyn Fn(&mut StorageTransaction<S>) -> Result<T>,
) -> Result<T> {
    let mut stx = StorageTransaction::new(storage);
    let res = tx(&mut stx)?;
    stx.prepare().commit(storage);
    Ok(res)
}

pub fn transactional_deps<S: Storage, A: Api, T>(
    deps: &mut Extern<S, A>,
    tx: &dyn Fn(&mut Extern<StorageTransaction<S>, A>) -> Result<T>,
) -> Result<T> {
    let c = StorageTransaction::new(&deps.storage);
    let mut stx_deps = Extern {
        storage: c,
        api: deps.api,
    };
    let res = tx(&mut stx_deps);
    if res.is_ok() {
        stx_deps.storage.prepare().commit(&mut deps.storage);
    } else {
        stx_deps.storage.rollback();
    }
    res
}

#[cfg(test)]
#[cfg(feature = "iterator")]
// iterator_test_suite takes a storage, adds data and runs iterator tests
// the storage must previously have exactly one key: "foo" = "bar"
// (this allows us to test StorageTransaction and other wrapped storage better)
//
// designed to be imported by other modules
fn iterator_test_suite<S: Storage>(store: &mut S) {
    // ensure we had previously set "foo" = "bar"
    assert_eq!(store.get(b"foo"), Some(b"bar".to_vec()));
    assert_eq!(store.range(..).count(), 1);

    // setup
    store.set(b"ant", b"hill");
    store.set(b"ze", b"bra");

    // open ended range
    {
        let iter = store.range(..);
        assert_eq!(3, iter.count());
        let mut iter2 = store.range(..);
        let first = iter2.next().unwrap();
        assert_eq!((b"ant".to_vec(), b"hill".to_vec()), first);
        let mut iter3 = store.range(..).rev();
        let last = iter3.next().unwrap();
        assert_eq!((b"ze".to_vec(), b"bra".to_vec()), last);
    }

    // closed range
    {
        let range = b"f".to_vec()..b"n".to_vec();
        let iter = store.range(range.clone());
        assert_eq!(1, iter.count());
        let mut iter2 = store.range(range.clone());
        let first = iter2.next().unwrap();
        assert_eq!((b"foo".to_vec(), b"bar".to_vec()), first);
    }

    // closed range reverse
    {
        let range = b"air".to_vec()..b"loop".to_vec();
        let iter = store.range(range.clone()).rev();
        assert_eq!(2, iter.count());
        let mut iter2 = store.range(range.clone()).rev();
        let first = iter2.next().unwrap();
        assert_eq!((b"foo".to_vec(), b"bar".to_vec()), first);
        let second = iter2.next().unwrap();
        assert_eq!((b"ant".to_vec(), b"hill".to_vec()), second);
    }

    // end open iterator
    {
        let range = b"f".to_vec()..;
        let iter = store.range(range.clone());
        assert_eq!(2, iter.count());
        let mut iter2 = store.range(range.clone());
        let first = iter2.next().unwrap();
        assert_eq!((b"foo".to_vec(), b"bar".to_vec()), first);
    }

    // end open iterator reverse
    {
        let range = b"f".to_vec()..;
        let iter = store.range(range.clone()).rev();
        assert_eq!(2, iter.count());
        let mut iter2 = store.range(range.clone()).rev();
        let first = iter2.next().unwrap();
        assert_eq!((b"ze".to_vec(), b"bra".to_vec()), first);
    }

    // start open iterator
    {
        let range = ..b"f".to_vec();
        let iter = store.range(range.clone());
        assert_eq!(1, iter.count());
        let mut iter2 = store.range(range.clone());
        let first = iter2.next().unwrap();
        assert_eq!((b"ant".to_vec(), b"hill".to_vec()), first);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::errors::Unauthorized;

    #[test]
    fn memory_storage_get_and_set() {
        let mut store = MemoryStorage::new();
        assert_eq!(None, store.get(b"foo"));
        store.set(b"foo", b"bar");
        assert_eq!(Some(b"bar".to_vec()), store.get(b"foo"));
        assert_eq!(None, store.get(b"food"));
    }

    #[test]
    #[cfg(feature = "iterator")]
    fn memory_storage_iterator() {
        let mut store = MemoryStorage::new();
        store.set(b"foo", b"bar");
        iterator_test_suite(&mut store);
    }

    #[test]
    #[cfg(feature = "iterator")]
    fn storage_transaction_iterator_empty_base() {
        let mut base = MemoryStorage::new();
        let mut check = StorageTransaction::new(&mut base);
        check.set(b"foo", b"bar");
        iterator_test_suite(&mut check);
    }

    #[test]
    #[cfg(feature = "iterator")]
    fn storage_transaction_iterator_with_base_data() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");
        let mut check = StorageTransaction::new(&mut base);
        iterator_test_suite(&mut check);
    }

    #[test]
    fn commit_writes_through() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");

        let mut check = StorageTransaction::new(&base);
        assert_eq!(check.get(b"foo"), Some(b"bar".to_vec()));
        check.set(b"subtx", b"works");
        check.prepare().commit(&mut base);

        assert_eq!(base.get(b"subtx"), Some(b"works".to_vec()));
    }

    #[test]
    fn storage_remains_readable() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");

        let mut stxn1 = StorageTransaction::new(&base);

        assert_eq!(stxn1.get(b"foo"), Some(b"bar".to_vec()));

        stxn1.set(b"subtx", b"works");
        assert_eq!(stxn1.get(b"subtx"), Some(b"works".to_vec()));

        // Can still read from base, txn is not yet committed
        assert_eq!(base.get(b"subtx"), None);

        stxn1.prepare().commit(&mut base);
        assert_eq!(base.get(b"subtx"), Some(b"works".to_vec()));
    }

    #[test]
    fn rollback_has_no_effect() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");

        let mut check = StorageTransaction::new(&mut base);
        assert_eq!(check.get(b"foo"), Some(b"bar".to_vec()));
        check.set(b"subtx", b"works");
        check.rollback();

        assert_eq!(base.get(b"subtx"), None);
    }

    #[test]
    fn ignore_same_as_rollback() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");

        let mut check = StorageTransaction::new(&mut base);
        assert_eq!(check.get(b"foo"), Some(b"bar".to_vec()));
        check.set(b"subtx", b"works");

        assert_eq!(base.get(b"subtx"), None);
    }

    #[test]
    fn transactional_works() {
        let mut base = MemoryStorage::new();
        base.set(b"foo", b"bar");

        // writes on success
        let res: Result<i32> = transactional(&mut base, &|store| {
            // ensure we can read from the backing store
            assert_eq!(store.get(b"foo"), Some(b"bar".to_vec()));
            // we write in the Ok case
            store.set(b"good", b"one");
            Ok(5)
        });
        assert_eq!(5, res.unwrap());
        assert_eq!(base.get(b"good"), Some(b"one".to_vec()));

        // rejects on error
        let res: Result<i32> = transactional(&mut base, &|store| {
            // ensure we can read from the backing store
            assert_eq!(store.get(b"foo"), Some(b"bar".to_vec()));
            assert_eq!(store.get(b"good"), Some(b"one".to_vec()));
            // we write in the Error case
            store.set(b"bad", b"value");
            Unauthorized.fail()
        });
        assert!(res.is_err());
        assert_eq!(base.get(b"bad"), None);
    }
}
