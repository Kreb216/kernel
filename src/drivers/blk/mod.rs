#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;

pub(crate) struct RequestQueue {}

pub(crate) struct VirtioBlkDriver {
	pub(super) request_vq: RequestQueue,
}

/// Error module of virtio blk device driver.
pub mod error {
	use thiserror::Error;

	/// Virtio Blk device error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioBlkError {
		#[error(
			"Virtio Blk device driver failed, for device {0:x}, due to driver not being implemented :D!"
		)]
		NoDevCfg(u16),
	}
}
