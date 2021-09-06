

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        // fails w/ overflow during type system BS???? lmao just disable
        .build_client(false)
        .compile(
            &["proto/dagstore.proto"],
                &["proto"],
        )?;
    Ok(())
}
