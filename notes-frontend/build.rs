use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;


// NOTE: currently only tested with hardcoded flat deploy dir
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("wasm_blobs");
    let _ = fs::create_dir(&dest_path); //.unwrap(); dir may already exist

    let current_dir = std::env::current_dir().unwrap();

    // TODO: figure out how to pass in --release flag conditionally (probably exists in env for build.rs)
    Command::new("cargo")
        .arg("web")
        .arg("deploy")
        .arg("--output")
        .arg(dest_path.to_str().unwrap())
        .current_dir(current_dir.join("wasm"))
        .output()
        .expect("failed to execute cargo web deploy");

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
        }).collect();

    // let mut f_contents = vec!["use phf::phf_map;".to_string()];


    let mut f_contents = blobs.iter().map(|(_, identifier, dest_path)| {
            format!(
                r#"static {}: &'static [u8] = include_bytes!("{}");"#,
                identifier, dest_path.to_str().unwrap()
            )
    }).collect::<Vec<String>>();

    // f_contents.append(&mut static_blobs);


    let mut hashmap: Vec<String> = vec!["pub static WASM: phf::Map<&'static str, &'static [u8]> = phf_map! {".to_string()];
    let mut hashmap_vals: Vec<String> = blobs.iter().map( |(src_path, identifier, _)| {
            format!(
                r#""{}" => {},"#,
                src_path, identifier
            )
    }).collect();
    hashmap.append(&mut hashmap_vals);
    hashmap.append(&mut vec!["};".to_string()]);

    f_contents.append(&mut hashmap);



    println!("output {:?}", &f_contents);

    f.write_all(&f_contents.join("\n").into_bytes())
    .unwrap();
    // unimplemented!("afaik only way to get println output from build.rs is to fail here");
}
