
pub mod storage {
    pub mod confirmed_block {
        include!(concat!(
            env!("OUT_DIR"),
            "/solana.storage.confirmed_block.rs"
        ));
    }
}
