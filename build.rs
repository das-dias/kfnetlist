fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("src/kfnetlist_schema")?;
    prost_build::Config::new()
        .out_dir("src/kfnetlist_schema")
        .compile_protos(
            &["kfnetlist-schema/circuit.proto"],
            &["kfnetlist-schema"],
        )?;

    Ok(())
}
