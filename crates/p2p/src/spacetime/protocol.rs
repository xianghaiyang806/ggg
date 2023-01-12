use futures::{channel::oneshot, future::BoxFuture, prelude::*};
use libp2p::{
	core::upgrade::{
		read_length_prefixed, write_length_prefixed, InboundUpgrade, OutboundUpgrade, UpgradeInfo,
	},
	swarm::NegotiatedSubstream,
};
use rmp_serde::{from_slice, to_vec_named};
use smallvec::SmallVec;
use std::{
	fmt,
	io::{self, ErrorKind},
};

use crate::spacetime::{RequestId, SpaceTime, SpaceTimeMessage};

/// Response substream upgrade protocol.
///
/// Receives a request and sends a response.
// #[derive(Debug)]
pub struct ResponseProtocol {
	pub(crate) protocols: SmallVec<[SpaceTime; 2]>,
	pub(crate) request_sender: oneshot::Sender<(RequestId, SpaceTimeMessage)>,
	pub(crate) response_receiver: oneshot::Receiver<SpaceTimeMessage>,
	pub(crate) request_id: RequestId,
}

impl UpgradeInfo for ResponseProtocol {
	type Info = SpaceTime;
	type InfoIter = smallvec::IntoIter<[Self::Info; 2]>;

	fn protocol_info(&self) -> Self::InfoIter {
		self.protocols.clone().into_iter()
	}
}

impl InboundUpgrade<NegotiatedSubstream> for ResponseProtocol {
	type Output = bool;
	type Error = io::Error;
	type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

	fn upgrade_inbound(self, mut io: NegotiatedSubstream, protocol: Self::Info) -> Self::Future {
		async move {
			let read = read_request(&protocol, &mut io);
			let request = read.await?;
			match self.request_sender.send((self.request_id, request)) {
				Ok(()) => {}
				Err(_) => {
					panic!("Expect request receiver to be alive i.e. protocol handler to be alive.",)
				}
			}

			if let Ok(response) = self.response_receiver.await {
				let write = write_response(&protocol, &mut io, response);
				write.await?;

				io.close().await?;
				// Response was sent. Indicate to handler to emit a `ResponseSent` event.
				Ok(true)
			} else {
				io.close().await?;
				// No response was sent. Indicate to handler to emit a `ResponseOmission` event.
				Ok(false)
			}
		}
		.boxed()
	}
}

/// Request substream upgrade protocol.
///
/// Sends a request and receives a response.
pub struct RequestProtocol {
	pub(crate) protocols: SmallVec<[SpaceTime; 2]>,
	pub(crate) request_id: RequestId,
	pub(crate) request: SpaceTimeMessage,
}

impl fmt::Debug for RequestProtocol {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("RequestProtocol")
			.field("request_id", &self.request_id)
			.finish()
	}
}

impl UpgradeInfo for RequestProtocol {
	type Info = SpaceTime;
	type InfoIter = smallvec::IntoIter<[Self::Info; 2]>;

	fn protocol_info(&self) -> Self::InfoIter {
		self.protocols.clone().into_iter()
	}
}

impl OutboundUpgrade<NegotiatedSubstream> for RequestProtocol {
	type Output = SpaceTimeMessage;
	type Error = io::Error;
	type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

	fn upgrade_outbound(self, mut io: NegotiatedSubstream, protocol: Self::Info) -> Self::Future {
		async move {
			let write = write_request(&protocol, &mut io, self.request);
			write.await?;
			io.close().await?;
			let read = read_response(&protocol, &mut io);
			let response = read.await?;
			Ok(response)
		}
		.boxed()
	}
}

// TODO: Merge these in above

async fn read_request<T>(_: &SpaceTime, io: &mut T) -> io::Result<SpaceTimeMessage>
where
	T: AsyncRead + Unpin + Send,
{
	// TODO: Restrict the size of request that can be read to prevent Dos attacks -> Decide on a logical value for it and timeout clients that keep trying to blow the limit!

	let buf = read_length_prefixed(io, 1_000_000).await?;
	if buf.is_empty() {
		return Err(io::Error::from(ErrorKind::UnexpectedEof));
	}
	// TODO: error handling
	Ok(from_slice(&buf).unwrap())
}

async fn read_response<T>(_: &SpaceTime, io: &mut T) -> io::Result<SpaceTimeMessage>
where
	T: AsyncRead + Unpin + Send,
{
	// TODO: Restrict the size of request that can be read to prevent Dos attacks -> Decide on a logical value for it and timeout clients that keep trying to blow the limit!

	let buf = read_length_prefixed(io, 1_000_000).await?;
	if buf.is_empty() {
		return Err(io::Error::from(ErrorKind::UnexpectedEof));
	}
	// TODO: error handling
	Ok(from_slice(&buf).unwrap())
}

async fn write_request<T>(_: &SpaceTime, io: &mut T, data: SpaceTimeMessage) -> io::Result<()>
where
	T: AsyncWrite + Unpin + Send,
{
	// TODO: error handling
	write_length_prefixed(io, to_vec_named(&data).unwrap().as_slice()).await?;
	io.close().await?;
	Ok(())
}

async fn write_response<T>(_: &SpaceTime, io: &mut T, data: SpaceTimeMessage) -> io::Result<()>
where
	T: AsyncWrite + Unpin + Send,
{
	// TODO: error handling
	write_length_prefixed(io, to_vec_named(&data).unwrap().as_slice()).await?;
	io.close().await?;
	Ok(())
}