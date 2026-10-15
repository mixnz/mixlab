//! Every MongoDB command.

use super::Transfer;
use super::{in_background, mongo_client, mongo_endpoint, reporter, tools_dir};
use crate::error::AppError;
use crate::modules::db::drivers::{dump, mongo, tools};
use crate::modules::db::models::ServerInfo;
use crate::modules::db::state::DbState;
use serde_json::Value;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

/// Writes a database out as a mongodump archive.
///
/// Reports on `transfer://progress` as the archive is written, measured against what the server
/// says the database's documents weigh.
#[tauri::command]
pub async fn mongo_dump(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    db: String,
    path: String,
) -> Result<(), AppError> {
    let tool = tools::require(tools::Tool::MongoDump, &tools_dir(&app)?)?;
    let client = mongo_client(&state, &id).await?;
    // What the archive is being measured against. A server that will not say leaves the dump to
    // run with a bar that moves without a number, which is not worth refusing to dump over.
    let documents: u64 = mongo::collection_stats(&client, &db)
        .await
        .map(|collections| collections.iter().map(|one| one.data_size).sum())
        .unwrap_or(0);
    let (uri, endpoint) = mongo_endpoint(&state, &id).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        let endpoint = endpoint.as_ref().map(|(host, port)| (host.as_str(), *port));
        mongo::dump::dump(
            &tool,
            &uri,
            endpoint,
            &db,
            &path,
            documents,
            &dump::Watch {
                report: &report,
                cancel: &|| cancelled.load(Ordering::Relaxed),
            },
        )
    })
    .await
}

/// Restores a mongodump archive into `db`, renaming its namespaces on the way in.
///
/// Reports on `transfer://progress` as the archive goes in, which like a MySQL restore is a count
/// rather than an estimate — the archive is fed to mongorestore byte by byte.
#[tauri::command]
pub async fn mongo_restore(
    app: AppHandle,
    state: State<'_, DbState>,
    id: String,
    db: String,
    path: String,
) -> Result<(), AppError> {
    let tool = tools::require(tools::Tool::MongoRestore, &tools_dir(&app)?)?;
    let (uri, endpoint) = mongo_endpoint(&state, &id).await?;
    let report = reporter(&app, &id);
    /* Registered for the length of the run and taken out however it ends, so the tab closing or
    the Cancel button has something to reach. */
    let transfer = Transfer::start(&state, &id);
    let cancelled = transfer.flag();
    in_background(move || {
        let endpoint = endpoint.as_ref().map(|(host, port)| (host.as_str(), *port));
        mongo::dump::restore(
            &tool,
            &uri,
            endpoint,
            &db,
            &path,
            &dump::Watch {
                report: &report,
                cancel: &|| cancelled.load(Ordering::Relaxed),
            },
        )
    })
    .await
}

/// Drops a database and every collection in it.
#[tauri::command]
pub async fn mongo_drop_database(
    state: State<'_, DbState>,
    id: String,
    db: String,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::drop_database(&client, &db).await
}

#[tauri::command]
pub async fn mongo_list_databases(
    state: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::list_databases(&client).await
}

#[tauri::command]
pub async fn mongo_server_info(
    state: State<'_, DbState>,
    id: String,
) -> Result<ServerInfo, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::server_info(&client).await
}

#[tauri::command]
pub async fn mongo_list_collections(
    state: State<'_, DbState>,
    id: String,
    db: String,
) -> Result<Vec<String>, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::list_collections(&client, &db).await
}

/// What every collection in the database weighs, for the workspace's Statistics tab.
#[tauri::command]
pub async fn mongo_collection_stats(
    state: State<'_, DbState>,
    id: String,
    db: String,
) -> Result<Vec<mongo::CollectionStats>, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::collection_stats(&client, &db).await
}

/// Creates an empty collection, for the sidebar's add button.
#[tauri::command]
pub async fn mongo_create_collection(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::create_collection(&client, &db, &collection).await
}

/// Renames a collection, for the sidebar's context menu.
#[tauri::command]
pub async fn mongo_rename_collection(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    new_name: String,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::rename_collection(&client, &db, &collection, &new_name).await
}

/// Drops a collection and every document in it, for the sidebar's context menu.
#[tauri::command]
pub async fn mongo_drop_collection(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::drop_collection(&client, &db, &collection).await
}

#[tauri::command]
pub async fn mongo_find(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    filter: String,
    limit: i64,
) -> Result<Vec<Value>, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::find(&client, &db, &collection, &filter, limit).await
}

#[tauri::command]
pub async fn mongo_collection_page(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    page: i64,
    page_size: i64,
    filters: Option<Vec<mongo::Filter>>,
) -> Result<mongo::CollectionPage, AppError> {
    let filters = filters.unwrap_or_default();
    let client = mongo_client(&state, &id).await?;
    mongo::collection_page(&client, &db, &collection, page, page_size, &filters).await
}

#[tauri::command]
pub async fn mongo_next_ids(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    count: i64,
) -> Result<Vec<Value>, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::next_ids(&client, &db, &collection, count).await
}

#[tauri::command]
pub async fn mongo_insert_documents(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    documents: Vec<Value>,
) -> Result<usize, AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::insert_documents(&client, &db, &collection, &documents).await
}

#[tauri::command]
pub async fn mongo_update_document(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    doc_id: Value,
    ops: mongo::DocUpdateOps,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::update_document(&client, &db, &collection, &doc_id, &ops).await
}

#[tauri::command]
pub async fn mongo_delete_document(
    state: State<'_, DbState>,
    id: String,
    db: String,
    collection: String,
    doc_id: Value,
) -> Result<(), AppError> {
    let client = mongo_client(&state, &id).await?;
    mongo::delete_document(&client, &db, &collection, &doc_id).await
}
