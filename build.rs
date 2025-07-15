fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .file_descriptor_set_path(std::env::var("OUT_DIR").unwrap() + "/arpc.descriptor.bin")
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(&["src/proto/arpc.proto"], &["src/proto"])?;

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .build_server(false)
        .file_descriptor_set_path(out_dir.join("geyser.descriptor.bin"))
        .out_dir(&out_dir)
        .compile_protos(
            &["src/proto/geyser.proto", "src/proto/solana-storage.proto"],
            &["src/proto"],
        )?;

    //nextblock
    tonic_build::configure()
        .build_server(false)
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(
            &["src/proto/nextblock_proto.proto"],
            &[
                "src/proto",
                "src/proto/protoc-gen-openapiv2",
                "src/proto/google",
            ],
        )?;

        //jito
        tonic_build::configure()
        .build_server(false)
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(
            &[
                "src/proto/jito/auth.proto",
                "src/proto/jito/block_engine.proto",
                "src/proto/jito/block.proto",
                "src/proto/jito/bundle.proto",
                "src/proto/jito/packet.proto",
                "src/proto/jito/shared.proto",
                "src/proto/jito/searcher.proto",
                "src/proto/jito/shredstream.proto",

            ],
            &[
                "src/proto/jito",
            ],
        )?;
    Ok(())
}
