use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

// NOTE: currently only tested with flat deploy dir
fn main() {
    // TODO: use to thread through RELEASE flag if needed
    // for (key, value) in std::env::vars() {
    //     println!("{}: {}", key, value);
    // }
    // panic!("dead");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("wasm_blobs_output_dir");
    let _ = fs::remove_dir_all(&dest_path); // may already exist, nuke if that is the case
    fs::create_dir(&dest_path).unwrap();

    let current_dir = std::env::current_dir().unwrap();

    // TODO: figure out how to pass in --release flag conditionally (probably exists in env for build.rs)
    let output = Command::new("cargo")
        .arg("web")
        .arg("deploy")
        .arg("--output")
        .arg(dest_path.to_str().unwrap())
        .current_dir(current_dir.join("wasm"))
        .output()
        .expect("failed to execute cargo web deploy");

    if !output.status.success() {
        std::io::stdout().write_all(&output.stdout).unwrap();
        std::io::stderr().write_all(&output.stderr).unwrap();
        panic!("failed to build wasm files")
    }

    let f_dest_path = Path::new(&out_dir).join("wasm_blobs.rs");
    let mut f = fs::File::create(&f_dest_path).unwrap();

    let blobs: Vec<(String, String, std::path::PathBuf)> = fs::read_dir(dest_path)
        .unwrap()
        .filter_map(|x| {
            let path = x.unwrap().path();
            if path.is_dir() {
                // TODO: fail? idk, will handle nested case later if needed
                None
            } else {
                let src = path.file_name().unwrap().to_str().unwrap().to_string();
                let identifier = src.clone().replace(".", "_").to_uppercase();
                Some((src, identifier, path))
            }
        })
        .collect();

    let mut f_contents = blobs
        .iter()
        .map(|(_, identifier, dest_path)| {
            format!(
                r#"static {}: &'static [u8] = include_bytes!("{}");"#,
                identifier,
                dest_path.to_str().unwrap()
            )
        })
        .collect::<Vec<String>>();

    let mut hashmap: Vec<String> =
        vec![ "lazy_static! {".to_string()
            , "static ref WASM: std::collections::HashMap<&'static str, &'static [u8]> = {".to_string()
            , "let mut m = std::collections::HashMap::new();".to_string()
            ];

    let mut hashmap_vals: Vec<String> = blobs
        .iter()
        .map(|(src_path, identifier, _)| format!(r#"m.insert("{}", {});"#, src_path, identifier))
        .collect();
    hashmap.append(&mut hashmap_vals);
    hashmap.append(&mut vec!["m".to_string()]);
    hashmap.append(&mut vec!["};".to_string()]);
    hashmap.append(&mut vec!["}".to_string()]);

    f_contents.append(&mut hashmap);

    f.write_all(&f_contents.join("\n").into_bytes()).unwrap();

    // TODO: figure out how to just register everything in wasm subdir
    println!("cargo:rerun-if-changed=wasm/src/main.rs");
    println!("cargo:rerun-if-changed=wasm/src/lib.rs");
    println!("cargo:rerun-if-changed=wasm/static/tree.css");
    // unimplemented!("afaik only way to get println output from build.rs is to fail here");
}
