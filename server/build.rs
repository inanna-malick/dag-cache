fn main() {
    tower_grpc_build::Config::new()
        .enable_server(true)
        .enable_client(false)
        .build(&["proto/ipfs_cache.proto"], &["proto"])
        .unwrap_or_else(|e| panic!("protobuf compilation failed: {}", e));
    println!("cargo:rerun-if-changed=proto/ipfs_cache.proto"); // magic cargo invocation via stdout? cool.
}
