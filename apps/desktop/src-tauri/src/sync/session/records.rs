//! What the engine asks of a session: stamping, heads, pulled pages, pushes, and their commits.

use std::collections::HashSet;
use std::sync::Arc;

use super::{now, token, Held, Kind, PulledPage, PushedChanges, SyncState, APP_VERSION};
use crate::error::AppError;
use crate::sync::crypto;
use crate::sync::engine;
use crate::sync::lend::{self, Incoming, Item};

impl SyncState {
    /// Stamp this machine's changes to `collection` before its pages are pulled, so the pull can
    /// weigh them against what it brings (D4). Nothing is sent.
    pub async fn notice(&self, collection: &str, items: Vec<Item>) -> Result<(), AppError> {
        let name = collection.to_owned();
        let items = Arc::new(items);
        self.with_session(|session| {
            let (name, items) = (name.clone(), items.clone());
            async move { lend::notice(&session.store, &session.keys, &name, &items, now()).await }
        })
        .await
    }

    /// Which of `collections` a full run must pull, in one request; every other one's cursor is
    /// moved past this machine's own writes. Plain names in, plain names out.
    pub async fn heads(&self, collections: Vec<String>) -> Result<Vec<String>, AppError> {
        let collections = Arc::new(collections);
        self.with_session(|session| {
            let collections = collections.clone();
            async move {
                let opaque: Vec<String> = collections
                    .iter()
                    .map(|name| crypto::opaque_id(&session.keys.id, name))
                    .collect();
                let stale: HashSet<String> = engine::stale(
                    &session.transport,
                    &session.store,
                    &session.limits,
                    &opaque,
                    APP_VERSION,
                )
                .await?
                .into_iter()
                .collect();
                Ok::<_, AppError>(
                    collections
                        .iter()
                        .zip(&opaque)
                        .filter(|(_, id)| stale.contains(*id))
                        .map(|(name, _)| name.clone())
                        .collect(),
                )
            }
        })
        .await
    }

    /// The next page of `collection`, opened, for its module to write.
    pub async fn pull_page(&self, collection: &str) -> Result<PulledPage, AppError> {
        let name = collection.to_owned();
        let (fetched, opened) = self
            .with_session(|session| {
                let name = name.clone();
                async move {
                    let opaque = crypto::opaque_id(&session.keys.id, &name);
                    let fetched =
                        engine::fetch(&session.transport, &session.store, &opaque, APP_VERSION)
                            .await?;
                    let opened = lend::incoming(
                        &session.store,
                        &session.keys,
                        &name,
                        &session.device_id,
                        &fetched.records,
                        fetched.resync && !fetched.more,
                    )
                    .await?;
                    // Named: an async block that uses `?` cannot infer its error type.
                    Ok::<_, AppError>((fetched, opened))
                }
            })
            .await?;
        let token = token();
        let more = fetched.more;
        self.inner.lock().await.held.insert(
            (Kind::Pull, name),
            Held {
                token: token.clone(),
                records: fetched.records.clone(),
                agreements: opened.agreements,
                fetched: Some(fetched),
                unmet: opened.unmet,
            },
        );
        Ok(PulledPage {
            token,
            changes: opened.changes,
            more,
        })
    }

    /// This machine's items for `collection`, pushed as whatever changed since they were last
    /// agreed. A lost conflict comes back as `replaced`, held for the module to write.
    pub async fn push(
        &self,
        collection: &str,
        items: Vec<Item>,
    ) -> Result<PushedChanges, AppError> {
        self.push_reporting(collection, items, || {}).await
    }

    /// [`push`](Self::push), calling `on_sending` just before anything leaves for the server — and
    /// never when nothing changed, which is most pushes. It is what the window's upload icon waits
    /// for: a push that sends nothing must not look like one that does.
    pub async fn push_reporting(
        &self,
        collection: &str,
        items: Vec<Item>,
        on_sending: impl Fn() + Sync,
    ) -> Result<PushedChanges, AppError> {
        let on_sending = &on_sending;
        let name = collection.to_owned();
        let items = Arc::new(items);
        let (accepted, replaced, records, agreements, error, needs_pull) = self
            .with_session(|session| {
                let (name, items) = (name.clone(), items.clone());
                async move {
                    // After a move the new account's store starts empty; pushed first, every record
                    // the move had copied came back as a conflict this machine won, and was
                    // written again.
                    let opaque = crypto::opaque_id(&session.keys.id, &name);
                    if !session.store.has_pulled(&opaque).await? {
                        return Ok::<_, AppError>((
                            0,
                            Incoming::default(),
                            Vec::new(),
                            Vec::new(),
                            None,
                            true,
                        ));
                    }
                    let (changes, agreed) =
                        lend::outgoing(&session.store, &session.keys, &name, &items, now()).await?;
                    if changes.is_empty() {
                        return Ok((0, Incoming::default(), Vec::new(), Vec::new(), None, false));
                    }
                    on_sending();
                    let pushed = engine::push(
                        &session.transport,
                        &session.store,
                        &session.limits,
                        &session.device_id,
                        changes,
                    )
                    .await?;
                    lend::settle_pushed(&session.store, &session.keys, &name, agreed, &pushed)
                        .await?;
                    // A dead token is refreshed and the push run again (`with_session`); what
                    // landed is agreed above, so the second run does not send it twice.
                    if let Some(error) = pushed
                        .error
                        .as_ref()
                        .filter(|error| error.code == "error.syncSignedOut")
                    {
                        return Err(error.clone());
                    }
                    let opened = lend::incoming(
                        &session.store,
                        &session.keys,
                        &name,
                        &session.device_id,
                        &pushed.superseded,
                        false,
                    )
                    .await?;
                    Ok((
                        pushed.accepted,
                        opened.changes,
                        pushed.superseded,
                        opened.agreements,
                        pushed.error,
                        false,
                    ))
                }
            })
            .await?;
        let token = if records.is_empty() {
            None
        } else {
            let token = token();
            self.inner.lock().await.held.insert(
                (Kind::Push, name),
                Held {
                    token: token.clone(),
                    records,
                    agreements,
                    fetched: None,
                    unmet: Vec::new(),
                },
            );
            Some(token)
        };
        Ok(PushedChanges {
            accepted,
            replaced,
            token,
            error,
            needs_pull,
        })
    }

    pub async fn commit_pull(
        &self,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        self.commit(Kind::Pull, collection, token, skipped).await
    }

    pub async fn commit_push(
        &self,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        self.commit(Kind::Push, collection, token, skipped).await
    }

    /// The module has written what `token` handed out, except the ids in `skipped`: record it, and
    /// for a page, move the cursor. No request is made.
    async fn commit(
        &self,
        kind: Kind,
        collection: &str,
        token: &str,
        skipped: Vec<String>,
    ) -> Result<(), AppError> {
        let (held, session) = {
            let mut inner = self.inner.lock().await;
            let key = (kind, collection.to_owned());
            let held = match inner.held.remove(&key) {
                Some(held) if held.token == token => held,
                // Handed out before a sign-in, or already recorded: recording it would agree on
                // something this session never read.
                Some(other) => {
                    inner.held.insert(key, other);
                    return Err(err!("error.syncPageStale"));
                }
                None => return Err(err!("error.syncPageStale")),
            };
            let session = inner
                .session
                .clone()
                .ok_or_else(|| err!("error.syncNotSignedIn"))?;
            (held, session)
        };
        lend::land(
            &session.store,
            &session.keys,
            collection,
            &held.records,
            held.agreements,
            &skipped,
            APP_VERSION,
        )
        .await?;
        if let Some(fetched) = &held.fetched {
            let opaque = crypto::opaque_id(&session.keys.id, collection);
            engine::commit(&session.store, &opaque, fetched, &held.unmet).await?;
        }
        Ok(())
    }
}
