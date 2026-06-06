use alloc::boxed::Box;
use super::ata::DriveType;
use super::traits::BlockDevice;

const MAX_DEVICES: usize = 8;

struct DeviceRegistry {
    devices: [Option<Box<dyn BlockDevice>>; MAX_DEVICES],
    count: usize,
}

impl DeviceRegistry {
    const fn new() -> Self {
        const NONE: Option<Box<dyn BlockDevice>> = None;
        DeviceRegistry {
            devices: [NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE],
            count: 0,
        }
    }

    fn register(&mut self, dev: Box<dyn BlockDevice>) -> usize {
        let id = self.count;
        if id < MAX_DEVICES {
            self.devices[id] = Some(dev);
            self.count += 1;
        }
        id
    }

    fn get_mut(&mut self, id: usize) -> Option<&mut dyn BlockDevice> {
        self.devices.get_mut(id)?.as_mut().map(|b| &mut **b as &mut dyn BlockDevice)
    }
}

static mut REGISTRY: DeviceRegistry = DeviceRegistry::new();

pub fn register_device(dev: Box<dyn BlockDevice>) -> usize {
    unsafe { REGISTRY.register(dev) }
}

pub fn get_device(id: usize) -> Option<&'static mut dyn BlockDevice> {
    let ptr: *mut dyn BlockDevice = match unsafe { REGISTRY.get_mut(id) } {
        Some(d) => d as *mut dyn BlockDevice,
        None => return None,
    };
    Some(unsafe { &mut *ptr })
}

pub fn device_count() -> usize {
    unsafe { REGISTRY.count }
}

pub fn flush_all() {
    let count = device_count();
    for id in 0..count {
        if let Some(dev) = get_device(id) {
            if dev.device_info().kind != DriveType::Atapi {
                let _ = dev.flush_cache();
            }
        }
    }
}
