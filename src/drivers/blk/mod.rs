#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;
use alloc::alloc::Allocator;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::u8;

use ::core::ptr;
use pci_types::InterruptLine;
use smallvec::SmallVec;
use virtio::blk::ConfigVolatileFieldAccess;
use virtio::{blk, le32, le64};
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
use crate::mm::device_alloc::DeviceAlloc;

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

		self.test_device();

		Ok(())
	}

	pub fn test_device(&mut self) -> Result<(), VirtioBlkError> {
		info!("Perform device test");

		let sector = le64::from_ne(2);

		let mut write_data = [0u8; 512];
		write_data[..4].copy_from_slice(b"test");

		info!("Writing to sector {sector:?}:");
		info!("Write bytes: {:02x?}", &write_data[..4]);

		self.send_req(virtio::blk::T::Out, sector, &write_data)?;

		let read_buf = [0u8; 512];

		let data = self
			.send_req(virtio::blk::T::In, sector, &read_buf)?
			.unwrap();

		info!("Read from sector {sector:?}:");
		info!("Data bytes: {:02x?}", &data[..4]);
		info!("Data text: {:?}", core::str::from_utf8(&data[..4]));

		Ok(())
	}

	pub fn alloc_req(
		&self,
		ty: virtio::blk::T,
		sector: le64,
		data: &[u8],
		dev_id: u16,
	) -> Result<Vec<u8, DeviceAlloc>, VirtioBlkError> {
		let layout = virtio::blk::Req::layout(data.len());
		let mem = DeviceAlloc
			.allocate_zeroed(layout)
			.map_err(|_| VirtioBlkError::BlkDevError(dev_id))?;

		let mut ptr = virtio::blk::Req::from_ptr(mem).ok_or(VirtioBlkError::BlkDevError(dev_id))?;

		let mut req_box = unsafe { Box::from_raw_in(ptr.as_mut(), DeviceAlloc) };

		req_box.ty = (ty as u32).into();
		req_box.reserved = le32::from_ne(0);
		req_box.sector = sector;

		info!("data.len(): {}", data.len());
		info!("req_box.data_mut().len(): {}", req_box.data_mut().len());
		req_box.data_mut().copy_from_slice(data); //TODO

		let raw_ptr = Box::into_raw_with_allocator(req_box).0.cast::<u8>();
		let req_as_vec = unsafe { Vec::from_raw_parts_in(raw_ptr, 1, 1, DeviceAlloc) };

		Ok(req_as_vec)
	}

	pub fn send_req(
		&mut self,
		ty: virtio::blk::T,
		sector: le64,
		data: &[u8],
	) -> Result<(Option<&[u8]>), VirtioBlkError> {
		if !matches!(ty, virtio::blk::T::Out | virtio::blk::T::In) {
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		let req = self.alloc_req(ty, sector, data, self.dev_cfg.dev_id)?;

		let mut vec = SmallVec::with_capacity(data.len());
		vec.push(BufferElem::Vector(req));

		let buffer_tkn = AvailBufferToken::new(SmallVec::new(), vec).unwrap();
		let mut request_result = self
			.request_vq
			.vq
			.as_mut()
			.ok_or(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?
			.dispatch_blocking(buffer_tkn, BufferType::Direct)
			.map_err(|_| VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?;

		// Check status byte
		let vec: Vec<u8, DeviceAlloc> = request_result.used_recv_buff.pop_front_vec().unwrap();

		let (ptr, len, cap, alloc) = Vec::into_raw_parts_with_alloc(vec);
		let nn_u8: NonNull<u8> =
			NonNull::new(ptr).ok_or(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?;
		let nn_slice: NonNull<[u8]> = NonNull::slice_from_raw_parts(nn_u8, len);
		let req_ptr: NonNull<virtio::blk::Req> = virtio::blk::Req::from_ptr(nn_slice)
			.ok_or(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id))?;

		let req: &virtio::blk::Req = unsafe { req_ptr.as_ref() };

		if *req.status().unwrap() != virtio::blk::S::OK as u8 {
			return Err(VirtioBlkError::BlkDevError(self.dev_cfg.dev_id));
		}

		if ty == virtio::blk::T::In {
			return Ok(Some(req.data()));
		}

		Ok(None)
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
