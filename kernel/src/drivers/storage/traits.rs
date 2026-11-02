use crate::drivers::storage::ata::{AtaError, DriveInfo};

pub trait BlockDevice: Send {
    fn read_sectors(&mut self, lba: u64, count: usize, buf: &mut [u8]) -> Result<(), AtaError>;
    fn write_sectors(&mut self, lba: u64, count: usize, buf: &[u8]) -> Result<(), AtaError>;
    fn flush_cache(&mut self) -> Result<(), AtaError>;
    fn total_sectors(&self) -> u64;
    fn device_info(&self) -> DriveInfo;
}
