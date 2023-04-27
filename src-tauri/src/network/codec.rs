use postcard::fixint::le;
use serde::{Serialize, Deserialize};
use tokio_util::codec::{Encoder, Decoder};
use bytes::{BytesMut, BufMut, Buf};
use uuid::Uuid;

use crate::{peer_id::PeerId, data::{ShareDirectorySignature, ShareDirectory, SharedFile}};

const MAX_MESSAGE_SIZE: usize = 1024 * 1024 * 100; // 100 MB
const LENGTH_MARKER_SIZE: usize = 4;

#[derive(Serialize, Deserialize, Debug)]
pub enum TcpMessage {
    RequestPeerId,
    SendPeerId(PeerId),

    Synchronize,
    SendDirectories(Vec<ShareDirectory>),

    DeleteFile {
        peer_id: PeerId,
        directory: ShareDirectorySignature,
        file: Uuid
    },

    AddedFiles {
        directory: ShareDirectorySignature,
        files: Vec<SharedFile>
    },

    SharedDirectory(ShareDirectory),
}

pub struct MessageCodec {

}

impl Encoder<TcpMessage> for MessageCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: TcpMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let encoded_message = match encode_message(item) {
            Ok(msg) => msg,
            Err(e) => return Err(e)
        };

        let len = encoded_message.len();
        let u32_len = u32::try_from(len).expect("large messages should have been handled by this point");

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
                format!("Message length {} is too large and should have been split into parts", length)
            ));
        }

        let full_length = length + LENGTH_MARKER_SIZE;
        if src.len() < full_length {
            src.reserve(full_length - src.len());

            return Ok(None);
        }

        let data = src[LENGTH_MARKER_SIZE..full_length].to_vec();
        src.advance(full_length);

        decode_message(data)
    }
}

fn decode_message(src: Vec<u8>) -> Result<Option<TcpMessage>, std::io::Error> {

    let result = match serde_json::from_slice(&src) {
        Ok(tcp_message) => {
            info!("decoded {:?}", tcp_message);

            Ok(Some(tcp_message))
        },
        Err(decode_err) => {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Could not parse message from received buffer: {}", decode_err),
            ))
        }
    };

    result
}

fn encode_message(src: TcpMessage) -> Result<Vec<u8>, std::io::Error> {
    info!("encoding {:?}", src);
    let enc = serde_json::to_vec(&src).expect("TcpMessage enum values should serialize without trouble");
    let len = enc.len() + LENGTH_MARKER_SIZE;

    if len > MAX_MESSAGE_SIZE {
        // split large messages into parts
        error!("Message too large to encode!");
        
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Message too large to encode")));
    }

    Ok(enc)
}