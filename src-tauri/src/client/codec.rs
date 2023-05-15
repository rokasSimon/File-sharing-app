use bytes::{Buf, BufMut, BytesMut};
use chrono::{DateTime, Utc};
use prost::Message;
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, Encoder};
use uuid::Uuid;

use crate::{
    data::{ShareDirectory, ShareDirectorySignature, SharedFile, PeerId}
};

use super::{DownloadError, protobuf::protobuf_types};


const MAX_MESSAGE_SIZE: usize = 1024 * 1024 * 100; // 100 MB
const LENGTH_MARKER_SIZE: usize = 4;

#[derive(Serialize, Deserialize, Debug)]
pub enum TcpMessage {
    RequestPeerId,
    ReceivePeerId(PeerId),

    Synchronize,
    ReceiveDirectories(Vec<ShareDirectory>),

    DeleteFile {
        peer_id: PeerId,
        directory: ShareDirectorySignature,
        file: Uuid,
    },

    AddedFiles {
        directory: ShareDirectorySignature,
        files: Vec<SharedFile>,
    },

    DownloadedFile {
        peer_id: PeerId,
        directory_identifier: Uuid,
        file_identifier: Uuid,
        date_modified: DateTime<Utc>,
    },

    StartDownload {
        download_id: Uuid,
        file_id: Uuid,
        dir_id: Uuid,
    },

    CancelDownload {
        download_id: Uuid,
    },

    ReceiveFilePart {
        download_id: Uuid,
        data: Vec<u8>,
    },

    ReceiveFileEnd {
        download_id: Uuid,
    },

    DownloadError {
        error: DownloadError,
        download_id: Uuid,
    },

    SharedDirectory(ShareDirectory),
    LeftDirectory {
        directory_identifier: Uuid,
        date_modified: DateTime<Utc>,
    },
}

pub struct MessageCodec {}

impl Encoder<TcpMessage> for MessageCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: TcpMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let encoded_message = match encode_protobuf(item) {
            Ok(msg) => msg,
            Err(e) => return Err(e),
        };

        let len = encoded_message.len();
        let u32_len =
            u32::try_from(len).expect("large messages should have been handled by this point");

        dst.reserve(len + LENGTH_MARKER_SIZE);
        dst.put_u32(u32_len);
        dst.put_slice(&encoded_message);

        Ok(())
    }
}

impl Decoder for MessageCodec {
    type Item = TcpMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < LENGTH_MARKER_SIZE {
            return Ok(None);
        }

        let mut length_bytes = [0u8; LENGTH_MARKER_SIZE];
        length_bytes.copy_from_slice(&src[..LENGTH_MARKER_SIZE]);
        let length = u32::from_be_bytes(length_bytes) as usize;

        if length > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Message length {} is too large and should have been split into parts",
                    length
                ),
            ));
        }

        let full_length = length + LENGTH_MARKER_SIZE;
        if src.len() < full_length {
            src.reserve(full_length - src.len());

            return Ok(None);
        }

        let data = src[LENGTH_MARKER_SIZE..full_length].to_vec();
        src.advance(full_length);

        decode_protobuf(data)
    }
}

pub fn decode_protobuf(data: Vec<u8>) -> Result<Option<TcpMessage>, std::io::Error> {
    let decoded_raw = protobuf_types::TcpMessage::decode(&data[..]);

    let decoded_raw = match decoded_raw {
        Ok(tcp_message) => match tcp_message.message {
            Some(msg) => msg,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Received empty message"),
                ))
            }
        },
        Err(decode_err) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Could not parse message from received buffer: {}",
                    decode_err
                ),
            ))
        }
    };

    let msg = decoded_raw.try_into()?;

    Ok(Some(msg))
}

pub fn encode_protobuf(src: TcpMessage) -> Result<Vec<u8>, std::io::Error> {
    match &src {
        TcpMessage::ReceiveFilePart {
            data: _,
            download_id: _,
        } => (),
        _ => info!("Encoding {:?}", src),
    }

    let msg = src.into();
    let mut raw_msg = protobuf_types::TcpMessage::default();
    raw_msg.message = Some(msg);
    let enc = protobuf_types::TcpMessage::encode_to_vec(&raw_msg);

    let len = enc.len() + LENGTH_MARKER_SIZE;

    if len > MAX_MESSAGE_SIZE {
        // split large messages into parts
        error!("Message too large to encode!");

        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Message too large to encode"),
        ));
    }

    Ok(enc)
}