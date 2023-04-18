use serde::{Serialize, Deserialize};
use tokio_util::codec::{Encoder, Decoder};
use bytes::{BytesMut, BufMut, Buf};

use crate::peer_id::PeerId;

const MAX_MESSAGE_SIZE: usize = 1024 * 1024 * 4; // 4 MB
const LENGTH_MARKER_SIZE: usize = 4;

#[derive(Serialize, Deserialize)]
pub enum TcpMessage {
    RequestPeerId,
    SendPeerId(PeerId),
    Part(u8, Vec<u8>)
}

pub struct MessageCodec {

}

impl Encoder<TcpMessage> for MessageCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: TcpMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let enc = postcard::to_stdvec_cobs(&item).expect("TcpMessage enum values should serialize without trouble");
        let len = enc.len() + LENGTH_MARKER_SIZE;

        if len > MAX_MESSAGE_SIZE {
            // split large messages into parts
            todo!();
        }

        let u32_len = u32::try_from(len).expect("large messages should have been handled by this point");

        dst.reserve(len);
        dst.put_u32(u32_len);
        dst.put_slice(&enc[..]);

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

        let mut length_bytes = [0; LENGTH_MARKER_SIZE];
        length_bytes.copy_from_slice(&src[..LENGTH_MARKER_SIZE]);
        let length = u32::from_le_bytes(length_bytes) as usize;

        if length > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Message length {} is too large and should have been split into parts", length)
            ));
        }

        if src.len() < length {
            src.reserve(length - src.len());

            return Ok(None);
        }

        let data = &src[LENGTH_MARKER_SIZE..length];
        let result = match postcard::from_bytes(data) {
            Ok(tcp_message) => Ok(Some(tcp_message)),
            Err(decode_err) => {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Could not parse message from received buffer: {}", decode_err),
                ))
            }
        };

        src.advance(length);

        result
    }
}