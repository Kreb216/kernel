#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;

use pci_types::InterruptLine;
use virtio::{le32, le64};

use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};

pub(crate) struct RequestQueue {}

//TODO: Comment
pub(crate) struct BlkDevCfg {
	pub capacity: le64,
	pub size_max: le32,
	pub seg_max: le32,
	pub geometry: virtio_blk_geometry,
	pub blk_size: le32,
	pub topology: virtio_blk_topology,
	pub writeback: u8,
	pub unused0: u8,
	pub num_queues: u8,
	pub max_discard_sectors: le32,
	pub max_discard_seg: le32,
	pub discard_sector_alignment: le32,
	pub max_write_zeroes_sectors: le32,
	pub max_write_zeroes_seg: le32,
	pub write_zeroes_may_unmap: u8,
	pub unused1: [u8; 3],
	pub max_secure_erase_sectors: le32,
	pub max_secure_erase_seg: le32,
	pub secure_erase_sector_alignment: le32,
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
