#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;

use pci_types::InterruptLine;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};

pub(crate) struct RequestQueue {}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct BlkDevCfg {
	pub raw: VolatileRef<'static, virtio::blk::Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::blk::F,
}

pub(crate) struct VirtioBlkDriver {
	pub(super) dev_cfg: BlkDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) irq: InterruptLine,

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
