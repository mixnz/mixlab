use super::filters::{split_list_parts, unquote, ListItem};
use crate::error::AppError;
use crate::modules::db::models::ServerInfo;
use base64::Engine;
use futures_util::stream::{self, StreamExt, TryStreamExt};
use mongodb::bson::spec::BinarySubtype;
use mongodb::bson::{
    doc, oid::ObjectId, Binary, Bson, DateTime as BsonDateTime, Decimal128, Document, Regex,
    Timestamp,
};
use mongodb::options::{ClientOptions, ServerAddress};
use mongodb::results::CollectionType;
use mongodb::{Client, Database};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::str::FromStr;

/// The first host a connection string points at. A tunnel needs a concrete address to forward
/// to, and with a URI that address is only knowable after parsing — a `mongodb+srv://` string
/// doesn't even contain it literally, it resolves to its hosts over DNS during the parse.
pub async fn first_endpoint(uri: &str) -> Result<(String, u16), AppError> {
    let opts = ClientOptions::parse(uri)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    match opts.hosts.first() {
        Some(ServerAddress::Tcp { host, port }) => Ok((host.clone(), port.unwrap_or(27017))),
        _ => Err(err!("error.mongoNoTcpHost")),
    }
}

/// `endpoint`, when given, replaces the host the URI names: an SSH tunnel listens on a local
/// port, so the address written in the connection string is no longer the one to dial.
pub async fn connect(uri: &str, endpoint: Option<(String, u16)>) -> Result<Client, AppError> {
    let mut opts = ClientOptions::parse(uri)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    opts.app_name = Some("MixLab".to_string());
    if let Some((host, port)) = endpoint {
        opts.hosts = vec![ServerAddress::Tcp {
            host,
            port: Some(port),
        }];
        // Only that one host is forwarded, so topology discovery would hand back the replica
        // set's own addresses — unreachable from this machine. Talk to the tunneled node
        // directly instead.
        opts.direct_connection = Some(true);
        opts.repl_set_name = None;
    }
    let client = Client::with_options(opts).map_err(|e| err!("error.mongo", message = e))?;
    // Fail fast on bad credentials/host instead of only failing on first query.
    client
        .database("admin")
        .run_command(doc! { "ping": 1 })
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    Ok(client)
}

/// Reads what the header shows about the server. `hostInfo` describes the machine the
/// server actually runs on — distribution, release and architecture, the same detail Redis
/// reports — but it needs a privilege managed deployments often withhold, so a server that
/// refuses it falls back to `buildInfo`, which only knows what the server was built for.
pub async fn server_info(client: &Client) -> Result<ServerInfo, AppError> {
    let admin = client.database("admin");
    let build = admin
        .run_command(doc! { "buildInfo": 1 })
        .await
        .map_err(|e| err!("error.mongo", message = e))?;

    let version = build.get_str("version").unwrap_or_default().to_string();
    let os = admin
        .run_command(doc! { "hostInfo": 1 })
        .await
        .ok()
        .and_then(|host| host_os(&host))
        .unwrap_or_else(|| build_os(&build));

    Ok(ServerInfo { version, os })
}

/// "Ubuntu 22.04 x86_64" out of `hostInfo`. Every part is optional, and a reply naming none
/// of them counts as no answer at all so the build environment can still fill the gap.
fn host_os(info: &Document) -> Option<String> {
    let os = info.get_document("os").ok();
    let parts: Vec<&str> = [
        os.and_then(|os| os.get_str("name").ok()),
        os.and_then(|os| os.get_str("version").ok()),
        info.get_document("system")
            .ok()
            .and_then(|system| system.get_str("cpuArch").ok()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect();

    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The OS the server was built for, out of `buildInfo` — "linux x86_64". The same pair MySQL
/// reports as `version_compile_os` and `version_compile_machine`.
fn build_os(info: &Document) -> String {
    let env = info.get_document("buildEnvironment").ok();
    let parts: Vec<&str> = [
        env.and_then(|env| env.get_str("target_os").ok()),
        env.and_then(|env| env.get_str("target_arch").ok()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect();

    parts.join(" ")
}

pub async fn list_databases(client: &Client) -> Result<Vec<String>, AppError> {
    client
        .list_database_names()
        .await
        .map_err(|e| err!("error.mongo", message = e))
}

pub async fn list_collections(client: &Client, db: &str) -> Result<Vec<String>, AppError> {
    let mut names = client
        .database(db)
        .list_collection_names()
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    names.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    Ok(names)
}

/// What one collection costs the server: the documents it holds and the bytes they and their
/// indexes take. The same four numbers the MySQL side reports for a table.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStats {
    pub name: String,
    pub rows: u64,
    /// The uncompressed size of the documents, `storageStats.size` — not `storageSize`, which is
    /// what they take on disk once WiredTiger has compressed them.
    pub data_size: u64,
    /// Every index on the collection together, `storageStats.totalIndexSize`.
    pub index_size: u64,
    /// The bytes of an average document — the data size over the count, which is zero for a
    /// collection holding nothing.
    pub avg_record_size: u64,
}

/// How many collections are measured at once. MongoDB has no one place that knows what a whole
/// database weighs, so this is a round trip per collection whatever else is done — running them
/// together is what keeps a database of a hundred collections from costing a hundred latencies in
/// a row, which is what it does over an SSH tunnel. Not raised past the driver's default pool of
/// ten connections: more in flight than that would only queue behind one another anyway.
const STATS_CONCURRENCY: usize = 8;

/// What every collection in the database weighs, listed by name.
///
/// Views are left out: one stores nothing of its own, and `$collStats` refuses to run on it at all.
pub async fn collection_stats(client: &Client, db: &str) -> Result<Vec<CollectionStats>, AppError> {
    let database = client.database(db);
    let mut specs = database
        .list_collections()
        .await
        .map_err(|e| err!("error.mongo", message = e))?;

    let mut names = Vec::new();
    while let Some(spec) = specs
        .try_next()
        .await
        .map_err(|e| err!("error.mongo", message = e))?
    {
        if !matches!(spec.collection_type, CollectionType::View) {
            names.push(spec.name);
        }
    }

    let mut stats: Vec<CollectionStats> = stream::iter(names)
        .map(|name| {
            let database = database.clone();
            async move { collection_size(&database, name).await }
        })
        .buffer_unordered(STATS_CONCURRENCY)
        .try_collect()
        .await?;

    // Sorted here rather than before the measuring: the answers come back in whatever order they
    // finish in, which is not the order they were asked for.
    stats.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(stats)
}

/// Measures one collection. `$collStats` reads what the collection itself keeps rather than any of
/// its documents, so this is cheap wherever the collection is large — it is the round trip that
/// costs, not the work at the far end.
async fn collection_size(database: &Database, name: String) -> Result<CollectionStats, AppError> {
    let mut cursor = database
        .collection::<Document>(&name)
        .aggregate(vec![doc! { "$collStats": { "storageStats": {} } }])
        .await
        .map_err(|e| err!("error.mongo", message = e))?;

    let mut rows = 0u64;
    let mut data_size = 0u64;
    let mut index_size = 0u64;
    // A sharded collection answers once per shard, each for the part of it that shard holds, so
    // the replies are added up rather than the first one taken.
    while let Some(reply) = cursor
        .try_next()
        .await
        .map_err(|e| err!("error.mongo", message = e))?
    {
        if let Ok(storage) = reply.get_document("storageStats") {
            rows += counter(storage, "count");
            data_size += counter(storage, "size");
            index_size += counter(storage, "totalIndexSize");
        }
    }

    Ok(CollectionStats {
        name,
        rows,
        data_size,
        index_size,
        // Worked out here rather than read from `avgObjSize`, which is per shard: the mean of the
        // shards' means is not the collection's, but total over total is.
        avg_record_size: data_size.checked_div(rows).unwrap_or(0),
    })
}

/// One of `$collStats`' numbers. Which BSON number it arrives as is the server's choice and varies
/// by field and by version, and a field an empty collection has no figure for is simply absent.
fn counter(stats: &Document, key: &str) -> u64 {
    match stats.get(key) {
        Some(Bson::Int32(value)) => (*value).max(0) as u64,
        Some(Bson::Int64(value)) => (*value).max(0) as u64,
        Some(Bson::Double(value)) => value.max(0.0) as u64,
        _ => 0,
    }
}

/// Creates an empty collection.
///
/// MongoDB makes one on the first write anyway, so this is about saying a collection exists before
/// there is anything to put in it — and about being told straight away when the name is taken or
/// not allowed, rather than at the first insert.
pub async fn create_collection(client: &Client, db: &str, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.collectionNameRequired"));
    }
    client
        .database(db)
        .create_collection(name)
        .await
        .map_err(|e| err!("error.mongo", message = e))
}

/// Drops a database and every collection in it.
pub async fn drop_database(client: &Client, db: &str) -> Result<(), AppError> {
    let db = db.trim();
    if db.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    client
        .database(db)
        .drop()
        .await
        .map_err(|e| err!("error.mongo", message = e))
}

/// Renames a collection within its database.
///
/// There is no per-database form of this: `renameCollection` is an admin command naming both
/// collections in full, so it takes privileges on the cluster rather than on the database — a
/// server that refuses says so, and that reason is what reaches the caller.
pub async fn rename_collection(
    client: &Client,
    db: &str,
    name: &str,
    new_name: &str,
) -> Result<(), AppError> {
    let name = name.trim();
    let new_name = new_name.trim();
    if name.is_empty() || new_name.is_empty() {
        return Err(err!("error.collectionNameRequired"));
    }
    client
        .database("admin")
        .run_command(doc! {
            "renameCollection": format!("{db}.{name}"),
            "to": format!("{db}.{new_name}"),
        })
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    Ok(())
}

/// Drops a collection and every document in it, along with its indexes.
pub async fn drop_collection(client: &Client, db: &str, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.collectionNameRequired"));
    }
    client
        .database(db)
        .collection::<Document>(name)
        .drop()
        .await
        .map_err(|e| err!("error.mongo", message = e))
}

pub async fn find(
    client: &Client,
    db: &str,
    collection: &str,
    filter_json: &str,
    limit: i64,
) -> Result<Vec<Value>, AppError> {
    let filter: Document = if filter_json.trim().is_empty() {
        doc! {}
    } else {
        let parsed: Value =
            serde_json::from_str(filter_json).map_err(|e| err!("error.mongo", message = e))?;
        mongodb::bson::to_document(&parsed).map_err(|e| err!("error.mongo", message = e))?
    };

    let coll = client.database(db).collection::<Document>(collection);
    let mut cursor = coll
        .find(filter)
        .limit(limit)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;

    let mut out = Vec::new();
    while let Some(doc) = cursor
        .try_next()
        .await
        .map_err(|e| err!("error.mongo", message = e))?
    {
        out.push(serde_json::to_value(&doc).map_err(|e| err!("error.mongo", message = e))?);
    }
    Ok(out)
}

/// Wraps an ambiguous BSON scalar as `{"$type": tag, "$value": repr}`. Types
/// with an unambiguous native JSON shape (String/Boolean/Null/Array/Document)
/// are never wrapped — the frontend tells them apart from typed scalars by
/// checking for this `$type`/`$value` pair, so leaving them bare keeps that
/// check simple and avoids colliding with a real field literally named
/// `$type` (astronomically unlikely, same caveat as MongoDB's own Extended
/// JSON).
fn wrap(tag: &str, value: Value) -> Value {
    json!({ "$type": tag, "$value": value })
}

/// Converts one BSON value into this app's typed-JSON shape (see `wrap`).
/// This is a from-scratch converter rather than `serde_json::to_value` on the
/// BSON document directly: we need full, explicit control over which shape
/// each ambiguous type takes (e.g. distinguishing Int32/Int64/Double, all of
/// which are plain numbers in JSON) so `json_to_bson` can invert it exactly.
pub fn bson_to_json(bson: &Bson) -> Value {
    match bson {
        Bson::Double(f) if f.is_finite() => wrap("Double", json!(f)),
        Bson::Double(f) => wrap("Double", Value::String(f.to_string())), // NaN/Infinity
        Bson::String(s) => Value::String(s.clone()),
        Bson::Array(arr) => Value::Array(arr.iter().map(bson_to_json).collect()),
        Bson::Document(doc) => Value::Object(
            doc.iter()
                .map(|(k, v)| (k.clone(), bson_to_json(v)))
                .collect(),
        ),
        Bson::Boolean(b) => Value::Bool(*b),
        Bson::Null => Value::Null,
        Bson::Int32(i) => wrap("Int32", json!(i)),
        Bson::Int64(i) => wrap("Int64", json!(i.to_string())),
        Bson::Decimal128(d) => wrap("Decimal128", json!(d.to_string())),
        // Always three fractional digits, the way JS `toISOString()` writes them. RFC3339 lets a
        // whole-second instant drop the fraction entirely, and that shorter spelling round-trips
        // through the editor as a different string, making an untouched date look edited.
        Bson::DateTime(dt) => wrap(
            "Date",
            json!(
                chrono::DateTime::from_timestamp_millis(dt.timestamp_millis())
                    .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .unwrap_or_else(|| dt.timestamp_millis().to_string())
            ),
        ),
        Bson::Timestamp(ts) => wrap("Timestamp", json!({ "t": ts.time, "i": ts.increment })),
        Bson::Binary(bin) => wrap(
            "Binary",
            json!({
                "base64": base64::engine::general_purpose::STANDARD.encode(&bin.bytes),
                "subType": u8::from(bin.subtype),
            }),
        ),
        Bson::ObjectId(oid) => wrap("ObjectId", json!(oid.to_hex())),
        Bson::RegularExpression(re) => wrap(
            "RegExp",
            json!({ "pattern": re.pattern.as_str(), "options": re.options.as_str() }),
        ),
        Bson::JavaScriptCode(code) => wrap("JavaScript", json!(code)),
        Bson::JavaScriptCodeWithScope(c) => wrap(
            "JavaScriptWithScope",
            json!({ "code": c.code, "scope": bson_to_json(&Bson::Document(c.scope.clone())) }),
        ),
        Bson::Symbol(s) => wrap("Symbol", json!(s)),
        Bson::Undefined => wrap("Undefined", Value::Null),
        Bson::MaxKey => wrap("MaxKey", Value::Null),
        Bson::MinKey => wrap("MinKey", Value::Null),
        // DbPointer's fields are pub(crate) in the bson crate — there is no
        // way to reconstruct one from outside the crate, so it is rendered
        // for display only; json_to_bson rejects writing this type back.
        Bson::DbPointer(dbp) => wrap("DbPointer", json!(format!("{dbp:?}"))),
    }
}

/// Inverse of `bson_to_json`. Recognizes the `{"$type","$value"}` shape for
/// ambiguous scalars; a plain JSON object with no `$type` key is treated as
/// a BSON subdocument.
pub fn json_to_bson(value: &Value) -> Result<Bson, AppError> {
    match value {
        Value::Null => Ok(Bson::Null),
        Value::Bool(b) => Ok(Bson::Boolean(*b)),
        Value::String(s) => Ok(Bson::String(s.clone())),
        Value::Array(arr) => arr
            .iter()
            .map(json_to_bson)
            .collect::<Result<_, _>>()
            .map(Bson::Array),
        Value::Number(n) => n
            .as_i64()
            .map(|i| {
                if i32::try_from(i).is_ok() {
                    Bson::Int32(i as i32)
                } else {
                    Bson::Int64(i)
                }
            })
            .or_else(|| n.as_f64().map(Bson::Double))
            .ok_or_else(|| err!("error.bsonInvalidNumber")),
        Value::Object(map) => match (map.get("$type"), map.get("$value")) {
            (Some(Value::String(tag)), Some(inner)) => decode_typed(tag, inner),
            _ => {
                let mut d = Document::new();
                for (k, v) in map {
                    d.insert(k.clone(), json_to_bson(v)?);
                }
                Ok(Bson::Document(d))
            }
        },
    }
}

fn decode_typed(tag: &str, v: &Value) -> Result<Bson, AppError> {
    match tag {
        "ObjectId" => v
            .as_str()
            .ok_or_else(|| err!("error.bsonObjectId"))
            .and_then(|s| ObjectId::parse_str(s).map_err(|_| err!("error.bsonObjectId")))
            .map(Bson::ObjectId),
        "Int32" => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(Bson::Int32)
            .ok_or_else(|| err!("error.bsonInt32Range")),
        "Int64" => v
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| v.as_i64())
            .map(Bson::Int64)
            .ok_or_else(|| err!("error.bsonInvalidValue", type = "Int64")),
        "Double" => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            .map(Bson::Double)
            .ok_or_else(|| err!("error.bsonInvalidValue", type = "Double")),
        "Decimal128" => v
            .as_str()
            .and_then(|s| Decimal128::from_str(s).ok())
            .map(Bson::Decimal128)
            .ok_or_else(|| err!("error.bsonInvalidValue", type = "Decimal128")),
        "Date" => v
            .as_str()
            .and_then(|s| BsonDateTime::parse_rfc3339_str(s).ok())
            .map(Bson::DateTime)
            .ok_or_else(|| err!("error.bsonDate")),
        "Binary" => {
            let b64 = v
                .get("base64")
                .and_then(Value::as_str)
                .ok_or_else(|| err!("error.bsonMissingField", type = "Binary", field = "base64"))?;
            let sub =
                v.get("subType").and_then(Value::as_u64).ok_or_else(
                    || err!("error.bsonMissingField", type = "Binary", field = "subType"),
                )? as u8;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| err!("error.mongo", message = e))?;
            Ok(Bson::Binary(Binary {
                subtype: BinarySubtype::from(sub),
                bytes,
            }))
        }
        "RegExp" => {
            let pattern = v.get("pattern").and_then(Value::as_str).ok_or_else(
                || err!("error.bsonMissingField", type = "RegExp", field = "pattern"),
            )?;
            let options = v.get("options").and_then(Value::as_str).unwrap_or("");
            Ok(Bson::RegularExpression(Regex {
                pattern: pattern.into(),
                options: options.into(),
            }))
        }
        "Timestamp" => {
            let t =
                v.get("t").and_then(Value::as_u64).ok_or_else(
                    || err!("error.bsonMissingField", type = "Timestamp", field = "t"),
                )? as u32;
            let i =
                v.get("i").and_then(Value::as_u64).ok_or_else(
                    || err!("error.bsonMissingField", type = "Timestamp", field = "i"),
                )? as u32;
            Ok(Bson::Timestamp(Timestamp {
                time: t,
                increment: i,
            }))
        }
        "JavaScript" => v
            .as_str()
            .map(|s| Bson::JavaScriptCode(s.to_string()))
            .ok_or_else(|| err!("error.bsonInvalidValue", type = "JavaScript")),
        "Symbol" => v
            .as_str()
            .map(|s| Bson::Symbol(s.to_string()))
            .ok_or_else(|| err!("error.bsonInvalidValue", type = "Symbol")),
        "Undefined" => Ok(Bson::Undefined),
        "MinKey" => Ok(Bson::MinKey),
        "MaxKey" => Ok(Bson::MaxKey),
        "JavaScriptWithScope" | "DbPointer" => Err(err!("error.bsonReadOnlyType", type = tag)),
        other => Err(err!("error.bsonUnknownType", type = other)),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct DocUpdateOps {
    #[serde(default)]
    pub set: Map<String, Value>,
    #[serde(default)]
    pub unset: Vec<String>,
    #[serde(default)]
    pub rename: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPage {
    pub documents: Vec<Value>,
    pub total: i64,
}

/// One condition on the documents a page is cut out of — the list's filter bar sends a list of
/// these, and they are ANDed together. `field` is a field path, dotted to reach into a
/// subdocument, and is spelled `column` on the wire so the bar can send the same shape whichever
/// database is behind it. `value` carries whatever the user typed, as text: the operator is what
/// says how to read it (a single value, a comma-separated list, a pair), and operators like
/// `exists` ignore it entirely.
#[derive(Debug, Deserialize)]
pub struct Filter {
    #[serde(rename = "column")]
    pub field: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Escapes the metacharacters out of text that is about to become part of a regex, so a value
/// with a `.` or a `*` in it is matched as itself. Only for the operators that build the pattern
/// (contains/starts with/ends with) — `regexp` hands the user's own pattern through untouched.
fn escape_regex(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if "\\^$.|?*+()[]{}/".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Reads the user's own regex out of the text box. `/pattern/flags` is understood — it is how a
/// regex is written everywhere else, and the only way to ask for one of Mongo's flags from a
/// single box — and anything else is taken as a bare pattern. Unknown flags are dropped rather
/// than passed on, so a `g` carried over from JavaScript habit doesn't have the server reject the
/// whole query.
fn parse_regex(raw: &str) -> Regex {
    if let Some(rest) = raw.strip_prefix('/') {
        if let Some(end) = rest.rfind('/') {
            let (pattern, flags) = rest.split_at(end);
            return Regex {
                pattern: pattern.to_string(),
                options: flags[1..]
                    .chars()
                    .filter(|c| "imsxu".contains(*c))
                    .collect(),
            };
        }
    }
    Regex {
        pattern: raw.to_string(),
        options: String::new(),
    }
}

/// Reads a BSON value out of the text a filter row holds.
///
/// This is the one thing a Mongo filter has to do that a SQL one doesn't. MySQL takes every value
/// as a bound string and coerces it against the column's own type; Mongo has no column type to
/// coerce against, and matches by exact BSON type — `{_id: "5"}` finds nothing in a collection
/// keyed by the number 5, and nothing at all in one keyed by ObjectIds. So the text is read for
/// what it looks like:
///
/// - `null`, `true`, `false` — those three values
/// - a whole number, else a decimal one
/// - 24 hex characters — an ObjectId, which is what `_id` usually holds
/// - an RFC 3339 timestamp — a date
/// - anything else — a string
///
/// Wrapping the value in quotes (`'5'`) turns all of that off and takes what is inside them as a
/// string, which is the way to reach a field that really does hold `"5"` as text.
fn parse_value(raw: &str) -> Bson {
    if let Some(text) = unquote(raw) {
        return Bson::String(text.to_string());
    }
    let trimmed = raw.trim();
    match trimmed {
        "null" => return Bson::Null,
        "true" => return Bson::Boolean(true),
        "false" => return Bson::Boolean(false),
        _ => {}
    }
    // A number has to have a digit in it: `inf` and `NaN` parse as f64, and nobody typing either
    // one into a filter box means the floating-point value.
    if trimmed.chars().any(|c| c.is_ascii_digit()) {
        if let Ok(i) = trimmed.parse::<i64>() {
            return match i32::try_from(i) {
                Ok(small) => Bson::Int32(small),
                Err(_) => Bson::Int64(i),
            };
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            return Bson::Double(f);
        }
    }
    if trimmed.len() == 24 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(oid) = ObjectId::from_str(trimmed) {
            return Bson::ObjectId(oid);
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Bson::DateTime(BsonDateTime::from_millis(dt.timestamp_millis()));
    }
    // As typed, not trimmed: a value that reached here is text, and the spaces around text are
    // part of it. The quoted form is there for when that is not what was meant.
    Bson::String(raw.to_string())
}

/// {@link parse_value} for one item of an `IN`/`BETWEEN` list, whose quotes were already taken
/// off during the split — the flag is all that is left of them.
fn parse_item(item: &ListItem) -> Bson {
    if item.quoted {
        Bson::String(item.text.clone())
    } else {
        parse_value(&item.text)
    }
}

/// Turns the filter rows into the query document the page is read through — one clause per row,
/// gathered under `$and` so that two conditions on the same field both survive (an object can
/// only hold one entry per key, and `{age: {...}, age: {...}}` would silently be one of them).
///
/// A row whose operator wants a value it wasn't given is dropped rather than matched literally:
/// the bar's opening `_id =` row must not become `{_id: ""}` before anything is typed into it.
fn build_filter(filters: &[Filter]) -> Result<Document, AppError> {
    let mut clauses: Vec<Document> = Vec::new();

    for filter in filters {
        let field = filter.field.trim();
        if field.is_empty() {
            continue;
        }
        // A leading `$` would make the field name read as a query operator, which is not a
        // document this bar has any way to mean.
        if field.starts_with('$') {
            return Err(err!("error.invalidFilterField", field = field));
        }
        let raw = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let condition: Bson = match operator {
            "eq" => doc! { "$eq": parse_value(raw) }.into(),
            "ne" => doc! { "$ne": parse_value(raw) }.into(),
            "gt" => doc! { "$gt": parse_value(raw) }.into(),
            "gte" => doc! { "$gte": parse_value(raw) }.into(),
            "lt" => doc! { "$lt": parse_value(raw) }.into(),
            "lte" => doc! { "$lte": parse_value(raw) }.into(),
            // Case-insensitive on purpose, so these read the same way as their SQL counterparts:
            // MySQL's default collation makes LIKE case-insensitive, and a filter bar that
            // matched differently depending on the workspace would be a trap. `regexp` below is
            // exempt — the user writing the pattern is the one who decides.
            "contains" | "notContains" | "startsWith" | "endsWith" => {
                let escaped = escape_regex(raw);
                let pattern = match operator {
                    "startsWith" => format!("^{escaped}"),
                    "endsWith" => format!("{escaped}$"),
                    _ => escaped,
                };
                let regex = Bson::RegularExpression(Regex {
                    pattern,
                    options: "i".to_string(),
                });
                if operator == "notContains" {
                    doc! { "$not": regex }.into()
                } else {
                    regex
                }
            }
            "regexp" => Bson::RegularExpression(parse_regex(raw)),
            "notRegexp" => doc! { "$not": Bson::RegularExpression(parse_regex(raw)) }.into(),
            "in" | "notIn" => {
                let items: Vec<Bson> = split_list_parts(raw).iter().map(parse_item).collect();
                if items.is_empty() {
                    continue;
                }
                let key = if operator == "in" { "$in" } else { "$nin" };
                doc! { key: items }.into()
            }
            "between" | "notBetween" => {
                let items = split_list_parts(raw);
                // Two bounds or nothing — one of them alone says nothing about a range.
                if items.len() < 2 {
                    continue;
                }
                let range = doc! { "$gte": parse_item(&items[0]), "$lte": parse_item(&items[1]) };
                if operator == "between" {
                    range.into()
                } else {
                    doc! { "$not": range }.into()
                }
            }
            // `$type` takes an alias (`string`, `int`, `date`) or the BSON type number; whichever
            // was typed is passed along, and the server is left to reject a name it doesn't know.
            "type" => {
                let alias = raw.trim();
                let wanted = match alias.parse::<i32>() {
                    Ok(n) => Bson::Int32(n),
                    Err(_) => Bson::String(alias.to_string()),
                };
                doc! { "$type": wanted }.into()
            }
            "exists" => doc! { "$exists": true }.into(),
            "notExists" => doc! { "$exists": false }.into(),
            // `{field: null}` would match the documents missing the field as well, which is the
            // opposite of what a schemaless collection needs to be able to ask.
            "isNull" => doc! { "$type": "null" }.into(),
            "isNotNull" => doc! { "$exists": true, "$not": { "$type": "null" } }.into(),
            "isEmpty" => doc! { "$eq": "" }.into(),
            "isNotEmpty" => doc! { "$exists": true, "$ne": "" }.into(),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        let mut clause = Document::new();
        clause.insert(field, condition);
        clauses.push(clause);
    }

    if clauses.is_empty() {
        return Ok(doc! {});
    }
    Ok(doc! { "$and": clauses })
}

/// Reads one page of a collection. `filters` narrows it down first, ANDed together; `total`
/// counts what is left after them, so the pager measures the filtered collection rather than the
/// whole one.
pub async fn collection_page(
    client: &Client,
    db: &str,
    collection: &str,
    page: i64,
    page_size: i64,
    filters: &[Filter],
) -> Result<CollectionPage, AppError> {
    let coll = client.database(db).collection::<Document>(collection);
    let page_size = page_size.clamp(1, 1000);
    let skip = (page.max(0) * page_size) as u64;
    let query = build_filter(filters)?;

    let total = coll
        .count_documents(query.clone())
        .await
        .map_err(|e| err!("error.mongo", message = e))? as i64;
    let mut cursor = coll
        .find(query)
        .skip(skip)
        .limit(page_size)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;

    let mut documents = Vec::new();
    while let Some(d) = cursor
        .try_next()
        .await
        .map_err(|e| err!("error.mongo", message = e))?
    {
        documents.push(bson_to_json(&Bson::Document(d)));
    }
    Ok(CollectionPage { documents, total })
}

/// Ids to prefill the `_id` of `count` new documents with.
///
/// Mongo has no auto-increment to read off: the id of a document that does not exist yet is
/// whatever the writer decides, and what every driver decides — including this one, when an
/// insert names no `_id` — is a freshly minted ObjectId. So that is the answer here too, and it
/// is a real "next id": an ObjectId leads with its creation timestamp, so the ones handed out
/// now sort after everything already in the collection.
///
/// The exception worth honouring is a collection keyed by numbers. Those are counted by hand
/// somewhere, and the only sensible next value is the highest plus one — so the highest `_id`
/// is read first, and its type decides. Anything else (strings, compound keys, an empty
/// collection) falls back to ObjectIds, since no scheme can be inferred from them.
pub async fn next_ids(
    client: &Client,
    db: &str,
    collection: &str,
    count: i64,
) -> Result<Vec<Value>, AppError> {
    let count = count.clamp(1, 100) as usize;
    let coll = client.database(db).collection::<Document>(collection);
    // Descending `_id` is the highest one under BSON's own type ordering, where every number
    // sorts below every string and ObjectId. In a numerically keyed collection that is exactly
    // the largest number; in a mixed one it is something else, and the fallback takes over.
    let highest = coll
        .find_one(doc! {})
        .sort(doc! { "_id": -1 })
        .projection(doc! { "_id": 1 })
        .await
        .map_err(|e| err!("error.mongo", message = e))?
        .and_then(|d| d.get("_id").cloned());

    let ids: Vec<Bson> = match highest {
        Some(Bson::Int32(n)) => (1..=count)
            .map(|i| Bson::Int32(n.saturating_add(i as i32)))
            .collect(),
        Some(Bson::Int64(n)) => (1..=count)
            .map(|i| Bson::Int64(n.saturating_add(i as i64)))
            .collect(),
        Some(Bson::Double(n)) => (1..=count).map(|i| Bson::Double(n + i as f64)).collect(),
        _ => (0..count)
            .map(|_| Bson::ObjectId(ObjectId::new()))
            .collect(),
    };
    Ok(ids.iter().map(bson_to_json).collect())
}

/// Writes new documents into a collection, in the order given.
///
/// Ordered rather than atomic: a transaction needs a replica set, which a standalone server is
/// not, so a failure partway through leaves the documents before it inserted. The caller is
/// expected to refetch the page afterwards — on failure as much as on success — so what landed
/// is what is on screen.
pub async fn insert_documents(
    client: &Client,
    db: &str,
    collection: &str,
    documents: &[Value],
) -> Result<usize, AppError> {
    if documents.is_empty() {
        return Ok(0);
    }
    let mut docs = Vec::with_capacity(documents.len());
    for (i, value) in documents.iter().enumerate() {
        // Built field by field rather than through `json_to_bson`, which would read a document
        // whose only keys are `$type` and `$value` as a wrapped scalar rather than as itself.
        let map = value
            .as_object()
            .ok_or_else(|| err!("error.documentNotObject", index = i + 1))?;
        let mut d = Document::new();
        for (k, v) in map {
            d.insert(
                k.clone(),
                json_to_bson(v)
                    .map_err(|e| err!("error.documentInvalid", index = i + 1).caused_by(e))?,
            );
        }
        docs.push(d);
    }

    let coll = client.database(db).collection::<Document>(collection);
    let result = coll
        .insert_many(docs)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    Ok(result.inserted_ids.len())
}

/// Applies a `$set`/`$unset`/`$rename` update to exactly one document,
/// identified by its (unchanging) `_id`. Unlike MySQL rows, Mongo documents
/// always have a natural single-field key, so no primary-key-discovery
/// fallback is needed.
pub async fn update_document(
    client: &Client,
    db: &str,
    collection: &str,
    doc_id: &Value,
    ops: &DocUpdateOps,
) -> Result<(), AppError> {
    if ops.set.is_empty() && ops.unset.is_empty() && ops.rename.is_empty() {
        return Ok(());
    }
    let coll = client.database(db).collection::<Document>(collection);
    let filter = doc! { "_id": json_to_bson(doc_id)? };

    let mut set_doc = Document::new();
    for (path, v) in &ops.set {
        set_doc.insert(path.clone(), json_to_bson(v)?);
    }
    let mut unset_doc = Document::new();
    for path in &ops.unset {
        unset_doc.insert(path.clone(), Bson::Int32(1));
    }
    let mut rename_doc = Document::new();
    for (old, new) in &ops.rename {
        rename_doc.insert(old.clone(), Bson::String(new.clone()));
    }

    let mut update = Document::new();
    if !set_doc.is_empty() {
        update.insert("$set", set_doc);
    }
    if !unset_doc.is_empty() {
        update.insert("$unset", unset_doc);
    }
    if !rename_doc.is_empty() {
        update.insert("$rename", rename_doc);
    }

    let result = coll
        .update_one(filter, update)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    if result.matched_count != 1 {
        return Err(err!(
            "error.documentsMatched",
            matched = result.matched_count
        ));
    }
    Ok(())
}

/// Deletes exactly one document, identified by its `_id`.
pub async fn delete_document(
    client: &Client,
    db: &str,
    collection: &str,
    doc_id: &Value,
) -> Result<(), AppError> {
    let coll = client.database(db).collection::<Document>(collection);
    let filter = doc! { "_id": json_to_bson(doc_id)? };

    let result = coll
        .delete_one(filter)
        .await
        .map_err(|e| err!("error.mongo", message = e))?;
    if result.deleted_count != 1 {
        return Err(err!(
            "error.documentsDeleted",
            deleted = result.deleted_count
        ));
    }
    Ok(())
}

/// The conversion the document editor is built on. Every value the user sees has been through
/// `bson_to_json`, and everything they save goes back through `json_to_bson` — so a type that
/// doesn't survive the round trip is a document quietly rewritten by having been looked at.
#[cfg(test)]
mod tests {
    use super::{bson_to_json, json_to_bson};
    use super::{build_filter, escape_regex, parse_regex, parse_value, Filter};

    fn filter(field: &str, operator: &str, value: Option<&str>) -> Filter {
        Filter {
            field: field.to_string(),
            operator: operator.to_string(),
            value: value.map(str::to_string),
        }
    }

    /// What the text in a filter box is read as. Mongo matches by exact BSON type, so this is the
    /// whole difference between finding a document and finding nothing: `{_id: "5"}` matches
    /// nothing in a collection keyed by the number 5.
    #[test]
    fn a_filter_value_is_read_as_the_bson_type_it_looks_like() {
        assert_eq!(parse_value("null"), Bson::Null);
        assert_eq!(parse_value("true"), Bson::Boolean(true));
        assert_eq!(parse_value("false"), Bson::Boolean(false));

        // Whole numbers stay small where they fit, because that is the type they were stored as.
        assert_eq!(parse_value("5"), Bson::Int32(5));
        assert_eq!(parse_value("-5"), Bson::Int32(-5));
        assert_eq!(parse_value("2147483648"), Bson::Int64(2_147_483_648));
        assert_eq!(parse_value("1.5"), Bson::Double(1.5));

        // A number needs a digit: `inf` and `NaN` parse as f64, and nobody typing either into a
        // filter box means the floating-point value.
        assert_eq!(parse_value("inf"), Bson::String("inf".to_string()));
        assert_eq!(parse_value("NaN"), Bson::String("NaN".to_string()));

        // 24 hex characters is what an ObjectId looks like, which is what `_id` usually holds.
        assert_eq!(
            parse_value("507f1f77bcf86cd799439011"),
            Bson::ObjectId(ObjectId::from_str("507f1f77bcf86cd799439011").unwrap()),
        );
        // 23 of them is a string, not a malformed id.
        assert_eq!(
            parse_value("507f1f77bcf86cd79943901"),
            Bson::String("507f1f77bcf86cd79943901".to_string()),
        );

        assert!(matches!(
            parse_value("2024-01-02T03:04:05Z"),
            Bson::DateTime(_)
        ));

        // Quoting turns all of it off, which is how a field that really does hold "5" is reached.
        assert_eq!(parse_value("'5'"), Bson::String("5".to_string()));
        assert_eq!(parse_value("'null'"), Bson::String("null".to_string()));

        // Text keeps the spaces around it — they are part of it, and the quoted form is there for
        // when they are not meant.
        assert_eq!(parse_value("  hi  "), Bson::String("  hi  ".to_string()));
    }

    /// `/pattern/flags` is how a regex is written everywhere else, and the only way to ask for one
    /// of Mongo's flags from a single text box.
    #[test]
    fn a_regex_is_read_with_its_flags_and_without_the_ones_mongo_has_no_use_for() {
        let plain = parse_regex("^a.*b$");
        assert_eq!(plain.pattern, "^a.*b$");
        assert_eq!(plain.options, "");

        let delimited = parse_regex("/^ab$/i");
        assert_eq!(delimited.pattern, "^ab$");
        assert_eq!(delimited.options, "i");

        // A `g` carried over from JavaScript habit is dropped rather than passed on, which would
        // have the server reject the whole query.
        assert_eq!(parse_regex("/ab/gimu").options, "imu");

        // The *last* slash closes it, so a pattern holding one still parses.
        let with_slash = parse_regex("/a/b/i");
        assert_eq!(with_slash.pattern, "a/b");
        assert_eq!(with_slash.options, "i");

        // An opening slash with no closing one is not a delimited regex; it is a pattern that
        // happens to start with a slash.
        assert_eq!(parse_regex("/ab").pattern, "/ab");
    }

    /// A value going into a built pattern is matched as itself, not as a pattern of its own.
    #[test]
    fn a_value_put_into_a_regex_is_escaped_first() {
        assert_eq!(escape_regex("a.b"), "a\\.b");
        assert_eq!(escape_regex("1+1"), "1\\+1");
        assert_eq!(escape_regex(".*"), "\\.\\*");
        assert_eq!(escape_regex("plain"), "plain");
    }

    /// The document a row turns into, and the rows that turn into nothing.
    #[test]
    fn a_filter_row_becomes_the_clause_it_means() {
        // Nothing to say yet: the bar opens with an empty row, which must not become `{_id: ""}`.
        assert_eq!(build_filter(&[]).unwrap(), doc! {});
        assert_eq!(
            build_filter(&[filter("  ", "eq", Some("x"))]).unwrap(),
            doc! {}
        );

        assert_eq!(
            build_filter(&[filter("age", "gte", Some("18"))]).unwrap(),
            doc! { "$and": [ { "age": { "$gte": 18i32 } } ] },
        );

        // Gathered under `$and` so that two conditions on one field both survive — an object
        // holds one entry per key, and `{age: .., age: ..}` would silently be one of them.
        let both = build_filter(&[
            filter("age", "gte", Some("18")),
            filter("age", "lte", Some("30")),
        ])
        .unwrap();
        assert_eq!(
            both,
            doc! { "$and": [ { "age": { "$gte": 18i32 } }, { "age": { "$lte": 30i32 } } ] },
        );

        // `startsWith` builds its own pattern, and escapes the value going into it.
        assert_eq!(
            build_filter(&[filter("name", "startsWith", Some("a.b"))]).unwrap(),
            doc! { "$and": [ { "name": Bson::RegularExpression(Regex {
                pattern: "^a\\.b".to_string(),
                options: "i".to_string(),
            }) } ] },
        );

        // `isNull` is not `{field: null}`, which would also match documents missing the field —
        // the opposite of what a schemaless collection needs to be able to ask.
        assert_eq!(
            build_filter(&[filter("nickname", "isNull", None)]).unwrap(),
            doc! { "$and": [ { "nickname": { "$type": "null" } } ] },
        );

        // A list of one bound is not a range, and an empty list is not a set.
        assert_eq!(
            build_filter(&[filter("age", "between", Some("18"))]).unwrap(),
            doc! {}
        );
        assert_eq!(
            build_filter(&[filter("age", "in", Some("  "))]).unwrap(),
            doc! {}
        );
    }

    /// What a filter refuses. A leading `$` would make the field name read as a query operator,
    /// which is not a document this bar has any way to mean.
    #[test]
    fn a_filter_refuses_an_operator_field_and_an_unknown_operator() {
        assert!(build_filter(&[filter("$where", "eq", Some("1"))]).is_err());
        assert!(build_filter(&[filter("age", "approximately", Some("18"))]).is_err());
    }

    use mongodb::bson::{
        doc, oid::ObjectId, spec::BinarySubtype, Binary, Bson, DateTime as BsonDateTime,
        Decimal128, Regex, Timestamp,
    };
    use serde_json::json;
    use std::str::FromStr;

    #[track_caller]
    fn round_trips(value: Bson) {
        let back = json_to_bson(&bson_to_json(&value)).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn the_types_json_spells_the_same_way_survive_the_round_trip() {
        round_trips(Bson::String("text".into()));
        round_trips(Bson::Boolean(true));
        round_trips(Bson::Null);
        round_trips(Bson::Array(vec![Bson::Int32(1), Bson::String("a".into())]));
        round_trips(Bson::Document(doc! { "a": 1_i32, "b": { "c": true } }));
    }

    /// The numbers are the reason this converter exists at all: Int32, Int64 and Double are one
    /// and the same thing in JSON, and a document read as one and written back as another is a
    /// document whose schema has silently changed.
    #[test]
    fn the_three_number_types_stay_apart() {
        round_trips(Bson::Int32(7));
        round_trips(Bson::Int64(i64::MAX));
        round_trips(Bson::Double(1.5));
        round_trips(Bson::Decimal128(Decimal128::from_str("1.250").unwrap()));

        // An Int64 too large for a double is carried as text, so it comes back exact rather than
        // rounded to the nearest representable value.
        let big = Bson::Int64(9_007_199_254_740_993);
        assert_eq!(json_to_bson(&bson_to_json(&big)).unwrap(), big);
    }

    #[test]
    fn the_types_json_has_no_spelling_for_survive_too() {
        round_trips(Bson::ObjectId(
            ObjectId::parse_str("65a1b2c3d4e5f60718293a4b").unwrap(),
        ));
        round_trips(Bson::DateTime(BsonDateTime::from_millis(1_700_000_000_123)));
        round_trips(Bson::Timestamp(Timestamp {
            time: 42,
            increment: 7,
        }));
        round_trips(Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![0, 1, 2, 255],
        }));
        round_trips(Bson::RegularExpression(Regex {
            pattern: "^a.*z$".into(),
            options: "i".into(),
        }));
        round_trips(Bson::JavaScriptCode("return 1".into()));
        round_trips(Bson::Symbol("sym".into()));
        round_trips(Bson::Undefined);
        round_trips(Bson::MinKey);
        round_trips(Bson::MaxKey);
    }

    /// A date is written with three fractional digits whatever the instant, so an untouched date
    /// reads back as the same string it was shown as rather than looking edited.
    #[test]
    fn a_whole_second_date_keeps_its_milliseconds() {
        let json = bson_to_json(&Bson::DateTime(BsonDateTime::from_millis(
            1_700_000_000_000,
        )));
        assert_eq!(json["$value"], json!("2023-11-14T22:13:20.000Z"));
    }

    /// A plain JSON object is a subdocument; only the `$type`/`$value` pair means a typed scalar.
    #[test]
    fn an_object_without_a_type_tag_is_a_subdocument() {
        let parsed = json_to_bson(&json!({ "name": "a", "nested": { "n": 1 } })).unwrap();
        assert_eq!(
            parsed,
            Bson::Document(doc! { "name": "a", "nested": { "n": 1_i32 } })
        );
    }

    /// Two types can be displayed but not reconstructed, and saying so is better than writing
    /// something else in their place.
    #[test]
    fn the_read_only_types_are_refused_on_the_way_back() {
        assert!(json_to_bson(&json!({ "$type": "DbPointer", "$value": "x" })).is_err());
        assert!(json_to_bson(&json!({ "$type": "JavaScriptWithScope", "$value": "x" })).is_err());
        assert!(json_to_bson(&json!({ "$type": "Nonsense", "$value": "x" })).is_err());
        assert!(json_to_bson(&json!({ "$type": "ObjectId", "$value": "not-hex" })).is_err());
    }
}
