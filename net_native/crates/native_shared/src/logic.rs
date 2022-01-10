use crate::*;
use crate::shared::DisconnectError;
use std::fmt::Debug;

use tokio::net::tcp::OwnedReadHalf;
use tokio::net::ToSocketAddrs;
use tokio::io::AsyncReadExt;

use turbulence::message_channels::{ChannelMessage, ChannelAlreadyRegistered};

pub async fn tcp_add_to_msg_queue(mut read_socket: OwnedReadHalf, unprocessed_messages_recv_queue: RecvQueue, conn_id: SuperConnectionHandle, max_packet_size: usize) -> std::io::Result<()>{
    let mut buffer: Vec<u8> = vec![0; max_packet_size];

    loop {
        let msg_len: usize = read_socket.read_u32().await?.try_into().unwrap();
        let channel_id = MessageChannelID::new(read_socket.read_u8().await?);

        if msg_len > max_packet_size {
            eprintln!("Received a packet that was too big!\nPacket was {} bytes", msg_len);
            break;
        }

        let msg_buffer = &mut buffer[..msg_len];

        let num_bytes_read = read_socket.read_exact(msg_buffer).await?;

        // If these differ, we read a corrupted message
        // TODO: Error something
        assert_eq!(msg_len, num_bytes_read);

        let mut key_val_pair = unprocessed_messages_recv_queue.entry(channel_id).or_insert(Vec::with_capacity(5));
        let messages = key_val_pair.value_mut();

        let byte_vec = msg_buffer.to_vec();

        messages.push((conn_id.clone(), byte_vec));

    }

    Ok(())

}

pub fn generate_message_bin<T>(message: &T, channel: &MessageChannelID) -> Result<Vec<u8>, bincode::Error> where T: ChannelMessage + Debug {
    let msg_bin = bincode::serialize(message)?;
    // Add one extra byte to the message length for the channel ID
    let msg_len: u32 = msg_bin.len().try_into().unwrap();

    // 4 bytes for the length, 1 byte for the channel ID, the rest for the actual message
    let mut final_message_bin = Vec::with_capacity(4 + 1 + msg_bin.len());

    final_message_bin.extend_from_slice(&msg_len.to_be_bytes());
    final_message_bin.push(channel.id);

    final_message_bin.extend_from_slice(&msg_bin);

    // Just a check to make sure we're properly encoding the message
    debug_assert_eq!(usize::try_from(u32::from_be_bytes(final_message_bin.as_slice()[..4].try_into().unwrap())).unwrap(), final_message_bin.as_slice()[5..].len());

    Ok(final_message_bin)
}

pub trait NativeResourceTrait {
    /// The actual setup of the network, whether it's connecting or listening
    fn setup(&mut self, tcp_addr: impl ToSocketAddrs + Send + Clone + 'static, udp_addr: impl ToSocketAddrs + Send + Clone + 'static, max_packet_size: usize);
    fn register_message(&self, channel: &MessageChannelID, mode: ChannelType) -> Result<(), ChannelAlreadyRegistered>;
    fn broadcast_message<T>(&self, message: &T, channel: &MessageChannelID) -> Result<(), SendMessageError> where T: ChannelMessage + Debug + Clone;
    fn send_message<T>(&self, message: &T, channel: &MessageChannelID, conn_id: &ConnID) -> Result<(), SendMessageError> where T: ChannelMessage + Debug;
    fn disconnect_from(&mut self, conn_id: &ConnID) -> Result<(), DisconnectError>;
    fn disconnect_from_all(&mut self);
}
