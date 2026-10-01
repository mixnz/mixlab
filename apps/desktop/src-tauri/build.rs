fn main() {
    // **The macOS executable exports nothing.** ld64 puts every external symbol of an executable
    // into its dyld export trie, and thin LTO leaves tens of thousands of Rust symbols external —
    // tokio's and mongodb's generic instances above all — so the trie went from 0.6 MB to 4.8 MB per
    // slice, and their names into the string table with it. Nothing loads MixLab as a plugin or
    // looks a symbol up in it, so the trie has no reader. A link argument here rather than in
    // `.cargo/config.toml`, because a `RUSTFLAGS` set by CI replaces config rustflags wholesale;
    // and for the binary only, since the `cdylib` this package also declares must keep its exports.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols");
    }
    tauri_build::build()
}
