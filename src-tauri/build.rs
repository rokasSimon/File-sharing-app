use std::io;

fn main() -> Result<(), io::Error> {
  prost_build::compile_protos(&["src/network/codec/tcpMessages.proto"], &["src/network/codec"])?;

  tauri_build::build();

  Ok(())
}
