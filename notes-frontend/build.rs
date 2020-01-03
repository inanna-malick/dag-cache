use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;


// NOTE: currently only tested with hardcoded flat deploy dir
// NOTE: still need to automate building deploy artifacts - still req's 
fn main() {
    println!("starting");
    let out_dir = env::var("OUT_DIR").unwrap();
    println!("out dir: {:?}", &out_dir);
    let dest_path = Path::new(&out_dir).join("wasm_blobs");
    let _ = fs::create_dir(&dest_path); //.unwrap(); dir may already exist

    let f_dest_path = Path::new(&out_dir).join("wasm_blobs.rs");
    let mut f = fs::File::create(&f_dest_path).unwrap();


    println!("here");

    let blobs: Vec<(String, String, std::path::PathBuf)> = fs::read_dir("/home/pk/dev/dag-store/notes-frontend/wasm/deploy")
        .unwrap()
        .filter_map(|x| {
            let path = x.unwrap().path();
            println!("got path {:?}", &path);
            if path.is_dir() {
                None
            } else {
                let out_path = Path::new(&dest_path).join(&path.file_name().unwrap());
                println!("got non-dir path {:?}, copying to {:?}", &path, &out_path);
                std::fs::copy(&path, &out_path).unwrap();

                let src = path.file_name().unwrap().to_str().unwrap().to_string();
                let identifier = src.clone().replace(".", "_").to_uppercase();
                Some((src, identifier, out_path))
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
