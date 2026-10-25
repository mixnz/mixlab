//! Copying a whole account to another server — D4b's steps 3 and 4 — with the ordinary read and the
//! ordinary batch write, and nothing the server has to know is a move.
//!
//! **The bytes cross unchanged**: the same opaque ids, the same nonce and ciphertext, the same
//! `updatedAt`. Every write says `If-None-Match: *`, and **`412 already-exists` is success** — so a
//! copy cut off halfway is finished by running it again, with nothing recorded in between.
//! Tombstones are not carried: the new server never held what they announce.

use std::collections::HashSet;
use std::future::Future;

use super::chunk::chunk;
use super::transport::{refusal, Remote, Transport};
use super::wire::{Capabilities, ErrorBody, Operation, Page, RecordBody};
use crate::error::AppError;

/// Something that pages through every record an account holds.
pub trait Source {
    /// `resync`: `since` came from a read that began at 0, so the server does not expire it.
    fn page_all(
        &self,
        since: i64,
        resync: bool,
    ) -> impl Future<Output = Result<Page, AppError>> + Send;
}

impl Source for Transport {
    async fn page_all(&self, since: i64, resync: bool) -> Result<Page, AppError> {
        Transport::page_all(self, since, resync).await
    }
}

/// Every live record `from` holds, written to `to` as it is. Returns the `(collection, id)` pairs
/// carried, which [`missing`] then checks for on the other side.
pub async fn copy_account<S: Source, R: Remote>(
    from: &S,
    to: &R,
    limits: &Capabilities,
) -> Result<Vec<(String, String)>, AppError> {
    let mut carried = Vec::new();
    let mut since = 0;
    loop {
        // A copy is a read from 0, so every later page says so: a tombstone reaped while it runs
        // must not end it halfway.
        let page = from.page_all(since, since > 0).await?;
        let mut operations = Vec::new();
        for record in page.records.into_iter().filter(|record| !record.deleted) {
            let (Some(nonce), Some(ciphertext)) = (record.nonce, record.ciphertext) else {
                return Err(err!("error.syncServerAnswerUnreadable"));
            };
            carried.push((record.collection.clone(), record.id.clone()));
            operations.push(Operation::Put {
                collection: record.collection,
                id: record.id,
                if_match: None,
                if_none_match: true,
                record: RecordBody {
                    updated_at: record.updated_at,
                    nonce,
                    ciphertext,
                },
            });
        }
        for batch in chunk(operations, limits)? {
            let results = to.batch(&batch).await?;
            if results.len() != batch.len() {
                return Err(err!("error.syncServerAnswerUnreadable"));
            }
            for result in results {
                match result.status {
                    200 | 201 | 412 => {}
                    _ => {
                        return Err(result
                            .error
                            .map(|error| refusal(&ErrorBody { error }))
                            .unwrap_or_else(|| err!("error.syncServerAnswerUnreadable")))
                    }
                }
            }
        }
        since = page.next_since;
        if !page.more {
            return Ok(carried);
        }
    }
}

/// How many of `wanted` the server `on` does not hold as a live record — step 5's comparison.
pub async fn missing<S: Source>(on: &S, wanted: &[(String, String)]) -> Result<usize, AppError> {
    let mut held = HashSet::new();
    let mut since = 0;
    loop {
        let page = on.page_all(since, since > 0).await?;
        held.extend(
            page.records
                .into_iter()
                .filter(|record| !record.deleted)
                .map(|record| (record.collection, record.id)),
        );
        since = page.next_since;
        if !page.more {
            break;
        }
    }
    Ok(wanted.iter().filter(|key| !held.contains(*key)).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::wire::{BatchResult, ErrorDetail, WireRecord};
    use std::sync::Mutex;

    fn record(collection: &str, id: &str, deleted: bool) -> WireRecord {
        WireRecord {
            collection: collection.into(),
            id: id.into(),
            version: 1,
            seq: 0,
            updated_at: 100,
            deleted,
            device: "d".into(),
            nonce: (!deleted).then(|| "n".into()),
            ciphertext: (!deleted).then(|| "x".into()),
        }
    }

    /// Pages of two, from a fixed list.
    struct Old(Vec<WireRecord>);

    impl Source for Old {
        async fn page_all(&self, since: i64, resync: bool) -> Result<Page, AppError> {
            // A server that has reaped refuses a later page that does not say it is a resync.
            assert!(since == 0 || resync, "a copy's later pages are a resync");
            let start = usize::try_from(since).unwrap();
            let records: Vec<_> = self.0.iter().skip(start).take(2).cloned().collect();
            let next = start + records.len();
            Ok(Page {
                records,
                next_since: i64::try_from(next).unwrap(),
                more: next < self.0.len(),
            })
        }
    }

    /// Takes every put it is sent; answers `412` for what it already holds, or `answer` if set.
    #[derive(Default)]
    struct New {
        held: Mutex<Vec<(String, String)>>,
        answer: Option<u16>,
    }

    impl Remote for New {
        async fn page(
            &self,
            _: &str,
            _: i64,
            _: bool,
        ) -> Result<crate::sync::transport::PageOutcome, AppError> {
            unreachable!("a copy writes with batch")
        }

        async fn batch(&self, operations: &[Operation]) -> Result<Vec<BatchResult>, AppError> {
            let mut held = self.held.lock().unwrap();
            Ok(operations
                .iter()
                .map(|operation| {
                    let Operation::Put { collection, id, .. } = operation else {
                        unreachable!("a copy only puts")
                    };
                    let key = (collection.clone(), id.clone());
                    let status = self
                        .answer
                        .unwrap_or(if held.contains(&key) { 412 } else { 201 });
                    held.push(key);
                    BatchResult {
                        status,
                        record: None,
                        error: (status >= 400).then(|| ErrorDetail {
                            code: if status == 412 {
                                "already-exists".into()
                            } else {
                                "quota-exceeded".into()
                            },
                            retry_after: None,
                        }),
                    }
                })
                .collect())
        }
    }

    impl Source for New {
        async fn page_all(&self, _: i64, _: bool) -> Result<Page, AppError> {
            let held = self.held.lock().unwrap();
            Ok(Page {
                records: held.iter().map(|(c, i)| record(c, i, false)).collect(),
                next_since: 0,
                more: false,
            })
        }
    }

    fn limits() -> Capabilities {
        Capabilities {
            protocol_versions: vec!["v1".into()],
            max_record_bytes: 1_000_000,
            max_batch_operations: 2,
            max_batch_bytes: 1_000_000,
            max_page_records: 2,
            account_quota_bytes: 20_971_520,
            tombstone_retention_days: 90,
            closing_on: None,
            features: vec![],
        }
    }

    #[tokio::test]
    async fn every_live_record_is_carried_and_no_tombstone() {
        let old = Old(vec![
            record("c", "a", false),
            record("c", "gone", true),
            record("d", "b", false),
            record("d", "c", false),
        ]);
        let new = New::default();
        let carried = copy_account(&old, &new, &limits()).await.unwrap();
        assert_eq!(carried.len(), 3);
        assert!(!carried.iter().any(|(_, id)| id == "gone"));
        assert_eq!(missing(&new, &carried).await.unwrap(), 0);
    }

    /// Resuming needs no bookkeeping: what already arrived answers `412`, and that is success.
    #[tokio::test]
    async fn a_copy_run_twice_is_the_same_copy() {
        let old = Old(vec![record("c", "a", false), record("c", "b", false)]);
        let new = New::default();
        copy_account(&old, &new, &limits()).await.unwrap();
        let again = copy_account(&old, &new, &limits()).await.unwrap();
        assert_eq!(again.len(), 2);
    }

    #[tokio::test]
    async fn a_refused_write_stops_the_copy() {
        let old = Old(vec![record("c", "a", false)]);
        let new = New {
            answer: Some(507),
            ..New::default()
        };
        let error = copy_account(&old, &new, &limits()).await.unwrap_err();
        assert_eq!(error.code, "error.syncQuotaExceeded");
    }

    #[tokio::test]
    async fn what_did_not_arrive_is_counted() {
        let new = New::default();
        let wanted = vec![("c".to_string(), "a".to_string())];
        assert_eq!(missing(&new, &wanted).await.unwrap(), 1);
    }
}
