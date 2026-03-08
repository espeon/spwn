fn main() {
    tonic_build::compile_protos("proto/agent.proto").unwrap();
}
