use phf::phf_map;

include!(concat!(env!("OUT_DIR"), "/wasm_blobs.rs"));

// TODO: fix return type, double pointer is not ideal
pub fn get_static_asset(s: &str) -> Option<&&'static [u8]> {
    WASM.get(s)
}
