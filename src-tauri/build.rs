fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!(
            "cargo:rustc-link-search=native={}",
            std::env::var("OUT_DIR").expect("OUT_DIR")
        );
    }
}
