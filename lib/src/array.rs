use crate::subscription::{subscription_key, YSubscription};
use crate::transaction::YrsTransaction;
use crate::{change::YrsChange, error::CodingError};
use std::cell::RefCell;
use std::fmt::Debug;
use std::sync::Arc;
use yrs::{Out as Value, Any, Array, ArrayRef, Doc, Observable, Transact};
use crate::doc::YrsCollectionPtr;

pub(crate) struct YrsArray(RefCell<ArrayRef>, Doc);

unsafe impl Send for YrsArray {}
unsafe impl Sync for YrsArray {}

impl YrsArray {
    pub(crate) fn new(value: ArrayRef, doc: Doc) -> Self {
        YrsArray(RefCell::from(value), doc)
    }
}
pub(crate) trait YrsArrayEachDelegate: Send + Sync + Debug {
    fn call(&self, value: String);
}

pub(crate) trait YrsArrayObservationDelegate: Send + Sync + Debug {
    fn call(&self, value: Vec<YrsChange>);
}

// unsafe impl Send for YrsArrayIterator {}
// unsafe impl Sync for YrsArrayIterator {}

// pub(crate) struct YrsArrayIterator {
//     inner: RefCell<ArrayIter<&'static YrsTransaction, YrsTransaction>>,
// }

// impl YrsArrayIterator {
//     pub(crate) fn next(&self) -> Option<String> {
//         let val = self.inner.borrow_mut().next();

//         match val {
//             Some(val) => {
//                 let mut buf = String::new();
//                 if let Value::Any(any) = val {
//                     any.to_json(&mut buf);
//                     Some(buf)
//                 } else {
//                     // @TODO: fix silly handling, it will just call it with nil if casting fails
//                     None
//                 }
//             }
//             None => None,
//         }
//     }
// }

impl YrsArray {
    // pub(crate) fn iter(&self, txn: &'static YrsTransaction) -> Arc<YrsArrayIterator> {
    //     let arr = self.0.borrow();
    //     Arc::new(YrsArrayIterator {
    //         inner: RefCell::new(arr.iter(txn)),
    //     })
    // }
    pub(crate) fn raw_ptr(&self) -> YrsCollectionPtr {
        let borrowed = self.0.borrow();
        YrsCollectionPtr::from(borrowed.as_ref())
    }

    pub(crate) fn each(
        &self,
        transaction: &YrsTransaction,
        delegate: Box<dyn YrsArrayEachDelegate>,
    ) {
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        let arr = self.0.borrow();
        arr.iter(tx).for_each(|val| {
            let mut buf = String::new();
            if let Value::Any(any) = val {
                any.to_json(&mut buf);
                delegate.call(buf);
            } else {
                // @TODO: fix silly handling, it will just call with empty string if casting fails
                delegate.call(buf);
            }
        });
    }

    pub(crate) fn get(
        &self,
        transaction: &YrsTransaction,
        index: u32,
    ) -> Result<String, CodingError> {
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();
        let arr = self.0.borrow();
        if let Some(value) = arr.get(tx, index) {
            let mut buf = String::new();
            if let Value::Any(any) = value {
                any.to_json(&mut buf);
                Ok(buf)
            } else {
                Err(CodingError::EncodingError)
            }
        } else {
            // Actually there is no element here, so it shouldn't be EncodingErro
            Err(CodingError::EncodingError)
        }
    }

    pub(crate) fn insert(&self, transaction: &YrsTransaction, index: u32, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();

        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.insert(tx, index, avalue);
    }

    pub(crate) fn insert_range(
        &self,
        transaction: &YrsTransaction,
        index: u32,
        values: Vec<String>,
    ) {
        let arr = self.0.borrow_mut();
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let add_values: Vec<Any> = values
            .into_iter()
            .map(|value| Any::from_json(value.as_str()).unwrap())
            .collect();

        arr.insert_range(tx, index, add_values)
    }

    pub(crate) fn length(&self, transaction: &YrsTransaction) -> u32 {
        let arr = self.0.borrow();
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        arr.len(tx)
    }

    pub(crate) fn push_back(&self, transaction: &YrsTransaction, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        self.0.borrow_mut().push_back(tx, avalue);
    }

    pub(crate) fn push_front(&self, transaction: &YrsTransaction, value: String) {
        let avalue = Any::from_json(value.as_str()).unwrap();

        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.push_front(tx, avalue);
    }

    pub(crate) fn remove(&self, transaction: &YrsTransaction, index: u32) {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.remove(tx, index)
    }

    pub(crate) fn remove_range(&self, transaction: &YrsTransaction, index: u32, len: u32) {
        let mut tx = transaction.transaction();
        let tx = tx.as_mut().unwrap();

        let arr = self.0.borrow_mut();
        arr.remove_range(tx, index, len)
    }

    pub(crate) fn observe(&self, delegate: Box<dyn YrsArrayObservationDelegate>) -> Arc<YSubscription> {
        let key = subscription_key();
        let id = { let reference = self.0.borrow(); let branch: &yrs::branch::Branch = reference.as_ref(); branch.id() };
        let doc = self.1.clone();
        self.0.borrow().observe(key.clone(), move |transaction, text_event| {
                let delta = text_event.delta(transaction);
                let result: Vec<YrsChange> =
                    delta.iter().map(|change| YrsChange::from(change)).collect();
                delegate.call(result)
            });
        Arc::new(YSubscription::new(move || {
            let tx = doc.transact();
            if let Some(branch) = id.get_branch(&tx) {
                ArrayRef::from(branch).unobserve(key);
            }
        }))
    }

    pub(crate) fn to_a(&self, transaction: &YrsTransaction) -> Vec<String> {
        let arr = self.0.borrow();
        let tx = transaction.transaction();
        let tx = tx.as_ref().unwrap();

        let arr = arr
            .iter(tx)
            .filter_map(|v| {
                let mut buf = String::new();
                if let Value::Any(any) = v {
                    any.to_json(&mut buf);
                    Some(buf)
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

        arr
    }
}
