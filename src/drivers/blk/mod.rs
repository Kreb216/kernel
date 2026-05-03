#![allow(dead_code)]

pub(crate) struct RequestQueue {}
use crate::arch::pci::PciConfigRegion;
use crate::drivers::blk::error::VirtioBlkError::NoDevCfg;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::VirtioError;

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

pub(crate) struct VirtioBlkDriver {
	pub(super) request_vq: RequestQueue,
}

impl VirtioBlkDriver {
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioBlkDriver, VirtioError> {
		Err(VirtioError::VirtioBlkDriver(NoDevCfg(device.device_id())))
	}
}
