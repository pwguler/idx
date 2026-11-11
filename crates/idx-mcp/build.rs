// idx-mcp links libduckdb.so dynamically (the official prebuilt, see
// .cargo/config.toml). A deployed binary finds it in its own directory.
fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
}
