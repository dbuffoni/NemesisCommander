//! Generates the gRPC code from `proto/`. Uses protox (a protobuf compiler
//! written in Rust), so building needs no `protoc` installation.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    let descriptors = protox::compile(["sc/agent/v1/agent.proto"], ["proto"])?;
    tonic_prost_build::configure()
        // File data stays in reference-counted buffers instead of Vec copies.
        .bytes(".")
        .compile_fds(descriptors)?;
    Ok(())
}
