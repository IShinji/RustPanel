fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc_path);

    let proto_files = [
        "../../proto/rustpanel/v1/accesslog.proto",
        "../../proto/rustpanel/v1/appstore.proto",
        "../../proto/rustpanel/v1/auth.proto",
        "../../proto/rustpanel/v1/audit.proto",
        "../../proto/rustpanel/v1/backup.proto",
        "../../proto/rustpanel/v1/capability.proto",
        "../../proto/rustpanel/v1/cluster.proto",
        "../../proto/rustpanel/v1/common.proto",
        "../../proto/rustpanel/v1/cron.proto",
        "../../proto/rustpanel/v1/db.proto",
        "../../proto/rustpanel/v1/dns.proto",
        "../../proto/rustpanel/v1/docker.proto",
        "../../proto/rustpanel/v1/fs.proto",
        "../../proto/rustpanel/v1/monitor.proto",
        "../../proto/rustpanel/v1/notification.proto",
        "../../proto/rustpanel/v1/proxy.proto",
        "../../proto/rustpanel/v1/rollback.proto",
        "../../proto/rustpanel/v1/security.proto",
        "../../proto/rustpanel/v1/site.proto",
        "../../proto/rustpanel/v1/ssl.proto",
        "../../proto/rustpanel/v1/system.proto",
        "../../proto/rustpanel/v1/terminal.proto",
        "../../proto/rustpanel/v1/toolbox.proto",
        "../../proto/rustpanel/v1/user.proto",
        "../../proto/rustpanel/v1/vsmtp.proto",
        "../../proto/rustpanel/v1/workload.proto",
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
