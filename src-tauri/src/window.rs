use std::path::PathBuf;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct OpenFile {
    pub file_path: PathBuf
}

#[tauri::command]
pub async fn open_file(
    message: OpenFile,
) -> Result<(), String> {

    info!("{:?}", message);

    let result = opener::open(&message.file_path);

    if let Err(e) = result {
        return Err(format!("Could not open file {}: {}", message.file_path.display(), e));
    }

    Ok(())
}