fn main() {
    tonic_build::compile_protos("proto/ipfs_cache.proto").unwrap();
}
