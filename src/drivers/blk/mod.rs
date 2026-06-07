#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;
use pci_types::InterruptLine;
use virtio::blk::ConfigVolatileFieldAccess;
use virtio::le64;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use super::virtio::virtqueue::VirtQueue;
use crate::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::Driver;
pub use crate::drivers::blk::error::VirtioBlkError;
use crate::drivers::virtio::ControlRegisters;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{AvailBufferToken, BufferElem, BufferType, Virtq};

pub(crate) struct RequestQueue {
	vq: Option<VirtQueue>,
}

impl RequestQueue {
	pub fn new() -> Self {
		Self { vq: None }
	}

	pub fn add(&mut self, vq: VirtQueue) {
		self.vq = Some(vq);
	}
}

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

impl Driver for VirtioBlkDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

impl VirtioBlkDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.acknowledge();

		#[cfg(not(feature = "pci"))]
		if status.contains(virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		#[cfg(feature = "pci")]
		if status.contains(virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioBlkError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///
	pub fn init_dev(&mut self) -> Result<(), VirtioBlkError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::blk::F::VERSION_1;
		let negotiated_features = self
			.com_cfg
			.control_registers()
			.negotiate_features(minimal_features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio block device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = negotiated_features;
		} else {
			error!("The device does not support our subset of features.");
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		//TODO: Device specific initialization
		// Create the queues and tell device about them
		self.request_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				0,
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));

		// Read block device size
		let block_size = self.dev_cfg.raw.as_ptr().capacity().read().to_ne() * 512;
		info!("Block device size: {block_size} bytes");

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}

	pub fn req(&mut self, req: virtio::blk::Req) {}

	pub fn write_req(&mut self, sector: le64, data: &[u8]) {
		let mut req = virtio::blk::Req {
			ty: virtio::blk::T::Out,
			reserved: 0,
			sector: sector,
			data_and_status: data,
		};
	}
}

/// Error module of virtio blk device driver.
pub mod error {
	use thiserror::Error;

	/// Virtio Blk device error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioBlkError {
		#[error("Virtio Blk device driver failed, for device {0:x}")]
		BlkDevError(u16),
	}
}
