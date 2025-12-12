use std::io::Result;

fn main() -> Result<()> {
    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
    config.compile_protos(
        &[
            "src/proto-public-api/public_api_types.proto",
            "src/proto-public-api/public_api_up.proto",
            "src/proto-public-api/public_api_down.proto",
        ],
        &["src/proto-public-api/"],
    )?;
    Ok(())
}
