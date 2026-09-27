//! What `/v1` puts on the wire, as Rust types (`docs/features/sync-protocol.md`).
//!
//! **Nothing here decides anything.** These are the shapes both servers answer with, spelled once
//! so the transport and the engine cannot disagree about the name of a field.

use serde::{Deserialize, Serialize};

/// A record as the server holds it (D3). `nonce` and `ciphertext` are absent on a tombstone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireRecord {
    pub collection: String,
    pub id: String,
    pub version: i64,
    pub seq: i64,
    pub updated_at: i64,
    pub deleted: bool,
    /// The device whose session wrote it, stamped by the server. D4's tie-break reads it.
    pub device: String,
    pub nonce: Option<String>,
    pub ciphertext: Option<String>,
}

/// One answer to `GET /v1/records`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub records: Vec<WireRecord>,
    pub next_since: i64,
    pub more: bool,
}

/// `POST /v1/records/heads` (T189).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heads {
    pub stale: Vec<String>,
    pub next_since: i64,
}

/// `GET /v1/capabilities`. Every number is the server's to choose and the client's to read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub protocol_versions: Vec<String>,
    pub max_record_bytes: u64,
    pub max_batch_operations: u64,
    pub max_batch_bytes: u64,
    pub max_page_records: u64,
    pub account_quota_bytes: u64,
    pub tombstone_retention_days: u64,
    pub closing_on: Option<i64>,
    pub features: Vec<String>,
}

/// The one shape every refusal takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    pub code: String,
    #[serde(default)]
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

/// What a `PUT` carries. `version`, `seq` and `device` are the server's to assign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordBody {
    pub updated_at: i64,
    pub nonce: String,
    pub ciphertext: String,
}

/// One entry of `POST /v1/records/batch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Operation {
    #[serde(rename_all = "camelCase")]
    Put {
        collection: String,
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        if_match: Option<i64>,
        #[serde(skip_serializing_if = "is_false")]
        if_none_match: bool,
        record: RecordBody,
    },
    #[serde(rename_all = "camelCase")]
    Delete {
        collection: String,
        id: String,
        if_match: i64,
        /// When the deletion was made: the tombstone's own time, which D4 weighs (T178c, C1).
        updated_at: i64,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One entry's outcome: exactly what the single-record route would have answered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BatchResult {
    pub status: u16,
    pub record: Option<WireRecord>,
    pub error: Option<ErrorDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BatchResponse {
    pub results: Vec<BatchResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_record_reads_the_way_d3_writes_it() {
        let record: WireRecord = serde_json::from_value(json!({
            "collection": "c".repeat(64), "id": "i".repeat(64), "version": 42, "seq": 901,
            "updatedAt": 1758300000, "deleted": false, "device": "d",
            "nonce": "n", "ciphertext": "x"
        }))
        .unwrap();
        assert_eq!(record.version, 42);
        assert_eq!(record.updated_at, 1_758_300_000);
        assert_eq!(record.device, "d");
        assert_eq!(record.ciphertext.as_deref(), Some("x"));
    }

    #[test]
    fn a_tombstone_has_no_ciphertext() {
        let record: WireRecord = serde_json::from_value(json!({
            "collection": "c", "id": "i", "version": 3, "seq": 9, "updatedAt": 1,
            "deleted": true, "device": "d"
        }))
        .unwrap();
        assert!(record.deleted);
        assert_eq!(record.nonce, None);
        assert_eq!(record.ciphertext, None);
    }

    #[test]
    fn a_creating_put_says_if_none_match_and_nothing_else() {
        let operation = Operation::Put {
            collection: "c".into(),
            id: "i".into(),
            if_match: None,
            if_none_match: true,
            record: RecordBody {
                updated_at: 5,
                nonce: "n".into(),
                ciphertext: "x".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&operation).unwrap(),
            json!({ "op": "put", "collection": "c", "id": "i", "ifNoneMatch": true,
                    "record": { "updatedAt": 5, "nonce": "n", "ciphertext": "x" } })
        );
    }

    #[test]
    fn an_updating_put_and_a_delete_say_if_match() {
        let put = Operation::Put {
            collection: "c".into(),
            id: "i".into(),
            if_match: Some(41),
            if_none_match: false,
            record: RecordBody {
                updated_at: 5,
                nonce: "n".into(),
                ciphertext: "x".into(),
            },
        };
        let delete = Operation::Delete {
            collection: "c".into(),
            id: "i".into(),
            if_match: 41,
            updated_at: 5,
        };
        assert_eq!(serde_json::to_value(&put).unwrap()["ifMatch"], json!(41));
        assert!(serde_json::to_value(&put)
            .unwrap()
            .get("ifNoneMatch")
            .is_none());
        assert_eq!(
            serde_json::to_value(&delete).unwrap(),
            json!({ "op": "delete", "collection": "c", "id": "i", "ifMatch": 41, "updatedAt": 5 })
        );
    }
}
