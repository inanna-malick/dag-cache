use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use ignore::Walk;

// NOTE: currently only tested with flat deploy dir
fn main() {
    let profile = std::env::var("PROFILE").expect("expected env var PROFILE for build.rs");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("wasm_blobs_output_dir");
    let _ = fs::remove_dir_all(&dest_path); // may already exist, nuke if that is the case
    fs::create_dir(&dest_path).unwrap();

    println!("dest path: {:?}", &dest_path);

    let current_dir = std::env::current_dir().unwrap();

    let mut cmd = Command::new("cargo");

    cmd
        .arg("web")
        .arg("deploy")
        .arg("--output")
        .arg(dest_path.to_str().unwrap());

    if profile == "release" {
        cmd.arg("--release");
    }

    cmd.current_dir(current_dir.join("wasm"));

    let output =
        cmd.output()
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

    let mut output_lines = blobs
        .iter()
        .map(|(_, identifier, dest_path)| {
            format!(
                r#"static {}: &'static [u8] = include_bytes!("{}");"#,
                identifier,
                dest_path.to_str().unwrap()
            )
        })
        .collect::<Vec<String>>();

    output_lines.append(&mut vec![ "lazy_static! {".to_string()
            , "static ref WASM: std::collections::HashMap<&'static str, &'static [u8]> = {".to_string()
            , "let mut m = std::collections::HashMap::new();".to_string()
            ]);

    let mut hashmap_entries: Vec<String> = blobs
        .iter()
        .map(|(src_path, identifier, _)| format!(r#"m.insert("{}", {});"#, src_path, identifier))
        .collect();
    output_lines.append(&mut hashmap_entries);
    output_lines.append(&mut vec!["m".to_string(), "};".to_string(), "}".to_string()]);


    f.write_all(&output_lines.join("\n").into_bytes()).unwrap();

    //register rerun-if-changed hooks for all wasm directory entries not in gitignore
    for result in Walk::new("wasm") {
        // Each item yielded by the iterator is either a directory entry or an
        // error, so either print the path or the error.
        match result {
            Ok(entry) => {
                if entry.metadata().unwrap().is_file() {
                    println!("cargo:rerun-if-changed={}", entry.path().display());
                }
            }
            Err(err) => panic!("error traversing wasm directory: {}", err),
        }
    }

    // panic!("afaik only way to get println output from build.rs is to fail here");
}
