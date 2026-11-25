fn main() {
    // Refresh Cargo's cached build warnings when upgrading from a CLI that set this variable.
    println!("cargo:rerun-if-env-changed=STATIC_VCRUNTIME");
    tauri_build::build()
}
