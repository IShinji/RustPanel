fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc_path);

    let proto_files = [
        "../../proto/rustpanel/v1/common.proto",
        "../../proto/rustpanel/v1/system.proto",
    ];
    let proto_includes = ["../../proto"];

    for path in proto_files {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=../../proto");

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&proto_files, &proto_includes)?;

    Ok(())
}
