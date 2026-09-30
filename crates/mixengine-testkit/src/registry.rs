//! A package index served over a real socket, signed with a real key.
//!
//! `docs/standards/testing.md` forbids network access in tests and CI blocks egress to enforce
//! it, so everything that reads an index has to read one from here. Deliberately **not** a fake
//! `Client`: the parts most worth testing are the signature check and the cache policy, and a
//! double that skipped either would be a test of the double.
//!
//! # It generates its own key, and that is the point
//!
//! A test cannot hold the production private key, so [`MockRegistry`] makes a fresh keypair per
//! instance and hands out its public half for `mixengine_core::index::Client::with` — named in
//! prose rather than linked, because this crate does not depend on `mixengine-core` and must not
//! start. That forces the key to be injectable in the product rather than hard-wired, and an
//! injectable key is what lets `MIXENGINE_INDEX_URL` point at a team mirror at all.
//!
//! The signature is produced by `minisign`, the signing half of the same author's pair of crates
//! that `minisign-verify` is the verifying half of. So a test proves the client accepts what
//! minisign actually produces, rather than what we believe it produces — which is the difference
//! that matters, since the format has a legacy variant the client refuses on purpose.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::io::Cursor;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use minisign::{KeyPair, SecretKey};

/// What the server is currently prepared to say.
#[derive(Debug)]
struct Published {
    document: Vec<u8>,
    signature: String,

    /// The second document, served at `/extensions.json` beside `/index.json` — the shape
    /// `mixengine_core::extensions::registry::Registry` reads. `None` until
    /// [`MockRegistry::publish_extensions`] is called, so a test that never touches the extension
    /// registry costs this struct nothing and a fetch against it 404s the way an unpublished path
    /// does for any other server.
    extensions: Option<(Vec<u8>, String)>,

    /// Index schema 2, encoded from the same value as `document` — roadmap task **T196**. `None`
    /// when that value is not an index at all, which is a registry started only for its assets.
    set: Option<Set>,

    /// Kinds whose file answers `404`.
    withheld: BTreeSet<String>,

    /// Whether any `/index-v2*` path answers. A mirror that copied only `index.json` is `false`.
    schema2: bool,

    /// Every path asked for, in order.
    requests: Vec<String>,

    reachable: bool,
    assets: BTreeMap<String, Vec<u8>>,
    cut_after: Option<usize>,
    ranges: Vec<Option<String>>,
}

/// Index schema 2 as this registry serves it: one signed root, one file per kind.
#[derive(Debug, Clone)]
struct Set {
    root: Vec<u8>,
    signature: String,

    /// Kind to its file's bytes.
    kinds: BTreeMap<String, Vec<u8>>,

    /// The path schema 2 composes for an artifact, to the path its bytes were published at.
    ///
    /// Schema 2 states no URL: an artifact is at `{base_url}/{kind}-{version}/…` and nowhere else.
    /// A fixture publishes its archive wherever it likes and writes that URL into a schema 1 index,
    /// so this is what lets every such fixture stay as it is.
    aliases: BTreeMap<String, String>,
}

/// The archive suffixes schema 2 can say.
const FORMATS: [&str; 3] = ["zip", "tar.zst", "tar.gz"];

/// Encode a schema 1 index as a root and a file per kind.
///
/// **This crate's own encoder**, written against the format and not against `mixengine-core` —
/// which this crate does not depend on and must not start to. So the client is tested against a
/// second implementation of the format rather than against itself.
///
/// [`None`] for a value with no `packages` or no `generated_at`.
///
/// # Panics
///
/// If an artifact's `url` ends in none of the three archive suffixes: schema 2 cannot say it, and
/// a fixture that needs one is a fixture whose file name is wrong.
fn encode(index: &serde_json::Value, address: SocketAddr, secret_key: &SecretKey) -> Option<Set> {
    use sha2::Digest as _;

    let generated_at = index.get("generated_at")?.as_str()?;
    let origin = format!("http://{address}");

    let mut by_kind: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
    for package in index.get("packages")?.as_array()? {
        let kind = package["kind"].as_str().expect("a package names its kind");
        by_kind.entry(kind.to_owned()).or_default().push(package);
    }

    let mut kinds = BTreeMap::new();
    let mut entries = serde_json::Map::new();
    let mut aliases = BTreeMap::new();

    for (kind, packages) in by_kind {
        let mut shapes = Vec::new();
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut encoded = Vec::new();

        for package in packages {
            let version = package["version"]
                .as_str()
                .expect("a package has a version");
            let stem = format!("{kind}-{version}");
            let mut artifacts = Vec::new();

            for artifact in package["artifacts"]
                .as_array()
                .expect("a package has artifacts")
            {
                let url = artifact["url"].as_str().expect("an artifact has a url");
                let format = FORMATS
                    .iter()
                    .find(|format| url.ends_with(&format!(".{format}")))
                    .unwrap_or_else(|| {
                        panic!("{stem} is published at {url}, which index schema 2 cannot say")
                    });
                let os = artifact["os"].as_str().expect("an artifact names its os");
                let arch = artifact["arch"]
                    .as_str()
                    .expect("an artifact names its arch");

                // Everything the artifact says that is not about its bytes is its shape.
                let mut shape = artifact
                    .as_object()
                    .expect("an artifact is an object")
                    .clone();
                for about_bytes in ["os", "arch", "url", "sha256", "size"] {
                    shape.remove(about_bytes);
                }
                let key = serde_json::to_string(&shape).expect("serialise a shape");
                let at = *seen.entry(key).or_insert_with(|| {
                    shapes.push(serde_json::Value::Object(shape));
                    shapes.len() - 1
                });

                if let Some(path) = url.strip_prefix(&origin) {
                    aliases.insert(
                        format!("/assets/{stem}/{stem}-{os}-{arch}.{format}"),
                        path.to_owned(),
                    );
                }

                artifacts.push(serde_json::json!({
                    "os": os, "arch": arch, "format": format,
                    "sha256": artifact["sha256"], "size": artifact["size"], "shape": at,
                }));
            }

            let mut entry = package.as_object().expect("a package is an object").clone();
            entry.remove("kind");
            entry.insert("artifacts".to_owned(), serde_json::Value::Array(artifacts));
            encoded.push(serde_json::Value::Object(entry));
        }

        let mut raw = serde_json::to_vec(&serde_json::json!({
            "schema": 2, "kind": kind, "shapes": shapes, "packages": encoded,
        }))
        .expect("serialise a kind file");
        raw.push(b'\n');

        entries.insert(
            kind.clone(),
            serde_json::json!({
                "sha256": format!("{:x}", sha2::Sha256::digest(&raw)),
                "size": raw.len(),
            }),
        );
        kinds.insert(kind, raw);
    }

    let mut root = serde_json::to_vec(&serde_json::json!({
        "schema": 2,
        "generated_at": generated_at,
        "base_url": format!("{origin}/assets"),
        "kinds": entries,
    }))
    .expect("serialise the root");
    root.push(b'\n');

    Some(Set {
        signature: sign(secret_key, &root),
        root,
        kinds,
        aliases,
    })
}

/// An in-process registry: one index, one signature, one switch for pulling the plug.
#[derive(Debug)]
pub struct MockRegistry {
    address: SocketAddr,
    public_key: String,
    secret_key: SecretKey,
    published: Arc<Mutex<Published>>,
}

impl MockRegistry {
    /// Start a registry serving `index`, and return once it is accepting connections.
    ///
    /// Bound to port 0 on loopback, so any number of these run at once without a port to allocate
    /// or a fixture to serialise on.
    ///
    /// # Panics
    ///
    /// If a keypair cannot be generated, the document cannot be signed, or the socket cannot be
    /// bound — each of which means the test environment is broken rather than the code under test.
    #[must_use]
    pub async fn start(index: &serde_json::Value) -> Self {
        let pair = KeyPair::generate_unencrypted_keypair().expect("generate a minisign keypair");
        let public_key = pair.pk.to_base64();

        let document = serde_json::to_vec_pretty(index).expect("serialise the index");
        let signature = sign(&pair.sk, &document);

        // Bound before anything is published: the schema 2 root states where artifacts live, and
        // that is this registry's own address.
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind a loopback port");
        let address = listener.local_addr().expect("read the bound port");

        let published = Arc::new(Mutex::new(Published {
            document,
            signature,
            extensions: None,
            set: encode(index, address, &pair.sk),
            withheld: BTreeSet::new(),
            schema2: true,
            requests: Vec::new(),
            reachable: true,
            assets: BTreeMap::new(),
            cut_after: None,
            ranges: Vec::new(),
        }));

        let serving = Arc::clone(&published);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let state = Arc::clone(&serving);
                tokio::spawn(async move {
                    let service = service_fn(move |request| answer(Arc::clone(&state), request));
                    // Errors here are a client that hung up mid-request, which several of these
                    // tests do on purpose.
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });

        Self {
            address,
            public_key,
            secret_key: pair.sk,
            published,
        }
    }

    /// The URL of the index document, which is what a client is pointed at.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/index.json", self.address)
    }

    /// The base64 public key this registry signs with.
    #[must_use]
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// Replace what is served, re-signed with the same key — `index.json` and the schema 2 set
    /// encoded from it, both whole.
    ///
    /// # Panics
    ///
    /// If the document cannot be serialised or signed.
    pub fn publish(&self, index: &serde_json::Value) {
        let document = serde_json::to_vec_pretty(index).expect("serialise the index");
        let signature = sign(&self.secret_key, &document);
        let set = encode(index, self.address, &self.secret_key);
        let mut published = self.published.lock().expect("the registry lock");
        published.document = document;
        published.signature = signature;
        published.set = set;
    }

    /// Serve the signature of a new root beside the old root — roadmap task **T196**.
    ///
    /// Not a state the publisher passes through — it uploads the signature last — and exactly what
    /// a client behind a cache that refreshed one file and not the other sees.
    ///
    /// # Panics
    ///
    /// If `index` is not an index, or no set is published yet.
    pub fn publish_signature_only(&self, index: &serde_json::Value) {
        let new = encode(index, self.address, &self.secret_key).expect("an index");
        let mut published = self.published.lock().expect("the registry lock");
        published
            .set
            .as_mut()
            .expect("a schema 2 set is published")
            .signature = new.signature;
    }

    /// Sign `root` and serve it as the schema 2 root, exactly as written; the kind files stay.
    ///
    /// For a root the encoder would never write: a schema this build cannot read, a kind whose
    /// name is not a name.
    ///
    /// # Panics
    ///
    /// If the root cannot be serialised or signed, or no set is published yet.
    pub fn publish_root(&self, root: &serde_json::Value) {
        let mut raw = serde_json::to_vec(root).expect("serialise the root");
        raw.push(b'\n');
        let signature = sign(&self.secret_key, &raw);
        let mut published = self.published.lock().expect("the registry lock");
        let set = published.set.as_mut().expect("a schema 2 set is published");
        set.root = raw;
        set.signature = signature;
    }

    /// Serve, for `kind`, bytes of the right length that do not hash to what the root says.
    ///
    /// # Panics
    ///
    /// If no such kind is published.
    pub fn corrupt_kind(&self, kind: &str) {
        let mut published = self.published.lock().expect("the registry lock");
        let bytes = published
            .set
            .as_mut()
            .and_then(|set| set.kinds.get_mut(kind))
            .expect("that kind is published");
        // The byte before the trailing newline is the document's closing brace.
        let at = bytes.len() - 2;
        bytes[at] = b' ';
    }

    /// Serve `kind`'s file one byte short — a transfer that stopped.
    ///
    /// # Panics
    ///
    /// If no such kind is published.
    pub fn truncate_kind(&self, kind: &str) {
        let mut published = self.published.lock().expect("the registry lock");
        published
            .set
            .as_mut()
            .and_then(|set| set.kinds.get_mut(kind))
            .expect("that kind is published")
            .pop();
    }

    /// Answer `404` for `kind`'s file — an upload that has not got that far.
    pub fn withhold_kind(&self, kind: &str) {
        self.published
            .lock()
            .expect("the registry lock")
            .withheld
            .insert(kind.to_owned());
    }

    /// Answer `404` for every schema 2 path — a mirror that copied `index.json` and nothing else.
    pub fn without_schema_2(&self) {
        self.published.lock().expect("the registry lock").schema2 = false;
    }

    /// Every path asked for so far, in order.
    ///
    /// What "nothing but the signature was requested" is asserted on: a client that fetched the
    /// whole index and got the right answer looks the same from the answer alone.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.published
            .lock()
            .expect("the registry lock")
            .requests
            .clone()
    }

    /// Start that list again.
    pub fn forget_requests(&self) {
        self.published
            .lock()
            .expect("the registry lock")
            .requests
            .clear();
    }

    /// Replace what is served at `/extensions.json`, signed with the same key as `/index.json`.
    ///
    /// The extension registry's own precedent for the one key: `.claude`'s note beside
    /// `registry::DEFAULT_URL` argues a compromise of it costs the package index either way, so a
    /// second key would separate nothing. Absent until this is called at least once.
    ///
    /// # Panics
    ///
    /// If the document cannot be serialised or signed.
    pub fn publish_extensions(&self, extensions: &serde_json::Value) {
        let document = serde_json::to_vec_pretty(extensions).expect("serialise the registry");
        let signature = sign(&self.secret_key, &document);
        let mut published = self.published.lock().expect("the registry lock");
        published.extensions = Some((document, signature));
    }

    /// Serve a document with a signature that does not cover it.
    ///
    /// The one tampering a real attacker gets to attempt against a client that checks nothing: the
    /// bytes are changed and the old signature is left in place. Both encodings: `index.json`, and
    /// the schema 2 root with its kind files — which is also the window a real publish has, where a
    /// new root sits beside the old signature until the signature is uploaded last.
    ///
    /// # Panics
    ///
    /// If the document cannot be serialised.
    pub fn publish_unsigned(&self, index: &serde_json::Value) {
        let document = serde_json::to_vec_pretty(index).expect("serialise the index");
        let set = encode(index, self.address, &self.secret_key);
        let mut published = self.published.lock().expect("the registry lock");
        published.document = document;
        if let (Some(old), Some(new)) = (published.set.as_mut(), set) {
            old.root = new.root;
            old.kinds = new.kinds;
            old.aliases = new.aliases;
        }
    }

    /// Stop answering, as a machine with no network does.
    ///
    /// Answers `503` rather than dropping the connection, so a test that exercises the offline path
    /// finishes immediately instead of waiting out a connect timeout. What the client sees either
    /// way is "the index could not be fetched", which is the only distinction it makes.
    pub fn unplug(&self) {
        self.published.lock().expect("the registry lock").reachable = false;
    }

    /// Answer again.
    pub fn plug(&self) {
        self.published.lock().expect("the registry lock").reachable = true;
    }

    /// Serve `bytes` at `path`, and answer with the URL that reaches them.
    ///
    /// This is what turns the registry from an index server into one an install can actually
    /// download from: the artifact a [`FakePackage`](crate::FakePackage) packed goes here, and the
    /// URL goes in the index document served beside it.
    ///
    /// **Range requests are honoured**, which is not decoration — a client that resumes a download
    /// is only resuming if the server is one that can be resumed from, and a mock that ignored the
    /// header would let a client which never sent one pass every test.
    pub fn publish_asset(&self, path: &str, bytes: Vec<u8>) -> String {
        self.published
            .lock()
            .expect("the registry lock")
            .assets
            .insert(path.to_owned(), bytes);

        format!("http://{}{path}", self.address)
    }

    /// End the next asset response after `bytes`, once.
    ///
    /// A connection dropped mid-file, which is the case resuming exists for. The body simply stops:
    /// the client is left holding a prefix, which is exactly what it is expected to notice and
    /// continue from. Consumed by the response it truncates, so the attempt after it succeeds.
    pub fn cut_next_response_after(&self, bytes: usize) {
        self.published.lock().expect("the registry lock").cut_after = Some(bytes);
    }

    /// The `Range` header of every asset request so far, in order, [`None`] where there was none.
    ///
    /// What a test asserts a resume on. "The file arrived eventually" is true of a client that
    /// downloaded it twice from the start.
    #[must_use]
    pub fn asset_ranges(&self) -> Vec<Option<String>> {
        self.published
            .lock()
            .expect("the registry lock")
            .ranges
            .clone()
    }
}

/// Sign `document` the way the publishing pipeline does.
fn sign(secret_key: &SecretKey, document: &[u8]) -> String {
    minisign::sign(
        None,
        secret_key,
        Cursor::new(document),
        Some("timestamp:0\tfile:index.json\thashed"),
        None,
    )
    .expect("sign the index")
    .into_string()
}

/// The document, the signature beside it, and whatever artifacts were published.
async fn answer(
    published: Arc<Mutex<Published>>,
    request: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (body, status) = {
        let mut published = published.lock().expect("the registry lock");
        published.requests.push(request.uri().path().to_owned());
        if !published.reachable {
            (Bytes::new(), StatusCode::SERVICE_UNAVAILABLE)
        } else {
            match request.uri().path() {
                "/index.json" => (Bytes::from(published.document.clone()), StatusCode::OK),
                "/index.json.minisig" => (
                    Bytes::from(published.signature.clone().into_bytes()),
                    StatusCode::OK,
                ),
                "/extensions.json" => match &published.extensions {
                    Some((document, _)) => (Bytes::from(document.clone()), StatusCode::OK),
                    None => (Bytes::new(), StatusCode::NOT_FOUND),
                },
                "/extensions.json.minisig" => match &published.extensions {
                    Some((_, signature)) => {
                        (Bytes::from(signature.clone().into_bytes()), StatusCode::OK)
                    }
                    None => (Bytes::new(), StatusCode::NOT_FOUND),
                },
                path if path.starts_with("/index-v2") => {
                    let served = published
                        .set
                        .as_ref()
                        .filter(|_| published.schema2)
                        .and_then(|set| match path {
                            "/index-v2.json" => Some(set.root.clone()),
                            "/index-v2.json.minisig" => Some(set.signature.clone().into_bytes()),
                            _ => path
                                .strip_prefix("/index-v2-")
                                .and_then(|name| name.strip_suffix(".json"))
                                .filter(|kind| !published.withheld.contains(*kind))
                                .and_then(|kind| set.kinds.get(kind).cloned()),
                        });

                    match served {
                        Some(bytes) => (Bytes::from(bytes), StatusCode::OK),
                        None => (Bytes::new(), StatusCode::NOT_FOUND),
                    }
                }
                path => {
                    // An artifact is asked for where schema 2 composes it and served from where
                    // the fixture published it.
                    let real = published
                        .set
                        .as_ref()
                        .and_then(|set| set.aliases.get(path).cloned())
                        .unwrap_or_else(|| path.to_owned());

                    match published.assets.get(&real).cloned() {
                        Some(asset) => asset_answer(&mut published, &request, asset),
                        None => (Bytes::new(), StatusCode::NOT_FOUND),
                    }
                }
            }
        }
    };

    Ok(Response::builder()
        .status(status)
        .body(Full::new(body))
        .expect("a response with no invalid headers"))
}

/// Serve an artifact, honouring `Range` and truncating when told to.
///
/// Only the open-ended `bytes=N-` form is understood, because that is the only one a resuming
/// download sends. Anything else is served whole, which is what a server that does not do ranges
/// does — and the client is expected to notice the `200` and start over rather than append.
fn asset_answer(
    published: &mut Published,
    request: &Request<hyper::body::Incoming>,
    asset: Vec<u8>,
) -> (Bytes, StatusCode) {
    let range = request
        .headers()
        .get(hyper::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    published.ranges.push(range.clone());

    let from = range
        .as_deref()
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.strip_suffix('-'))
        .and_then(|value| value.parse::<usize>().ok());

    let (body, status) = match from {
        Some(from) if from >= asset.len() => (Vec::new(), StatusCode::RANGE_NOT_SATISFIABLE),
        Some(from) => (asset[from..].to_vec(), StatusCode::PARTIAL_CONTENT),
        None => (asset, StatusCode::OK),
    };

    // Truncation is applied after the range, so a cut second response is a cut *resume*.
    let body = match published.cut_after.take() {
        Some(cut) if cut < body.len() => body[..cut].to_vec(),
        _ => body,
    };

    (Bytes::from(body), status)
}
