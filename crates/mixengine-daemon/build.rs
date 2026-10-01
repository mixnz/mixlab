fn main() {
    // **Every Linux `mixengined` carries a GNU build-id** (T91a). A crash report records offsets
    // into the executable and names the build they belong to, and `scripts/symbolize.mjs` refuses a
    // symbol file whose build-id is not the report's. The toolchains this repository builds with
    // write one by default, which is why the release binaries already carry it; asking for it here
    // keeps that true on a toolchain whose default is otherwise. `ld64` always writes `LC_UUID`, and
    // MSVC's linker always writes the CodeView record, so the other two need nothing.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,--build-id");
    }
}
