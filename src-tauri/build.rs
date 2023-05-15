use std::io;

fn main() -> Result<(), io::Error> {
  prost_build::compile_protos(&["src/client/tcpMessages.proto"], &["src/client"])?;

  tauri_build::build();

  Ok(())
}
