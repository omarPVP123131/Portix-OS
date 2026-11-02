use alloc::boxed::Box;
use crate::arch::Spinlock;
use super::ata::DriveType;
use super::traits::BlockDevice;

const MAX_DEVICES: usize = 8;

struct DeviceRegistry {
    devices: [Option<*mut dyn BlockDevice>; MAX_DEVICES],
    count: usize,
}

impl DeviceRegistry {
    const fn new() -> Self {
        const NONE: Option<*mut dyn BlockDevice> = None;
        DeviceRegistry {
            devices: [NONE; MAX_DEVICES],
            count: 0,
        }
    }

    fn register(&mut self, dev: Box<dyn BlockDevice>) -> Option<usize> {
        let id = self.count;
        if id < MAX_DEVICES {
            let leaked = Box::leak(dev) as *mut dyn BlockDevice;
            self.devices[id] = Some(leaked);
            self.count += 1;
            Some(id)
        } else {
            None
        }
    }

    fn get_static(&self, id: usize) -> Option<&'static mut dyn BlockDevice> {
        let ptr = (*self.devices.get(id)?)?;
        Some(unsafe { &mut *ptr })
    }

    fn len(&self) -> usize { self.count }
}

// SAFETY: El kernel PORTIX es single-threaded / cooperativo.
unsafe impl Send for DeviceRegistry {}
unsafe impl Sync for DeviceRegistry {}

static REGISTRY: Spinlock<DeviceRegistry> = Spinlock::new(DeviceRegistry::new());

pub fn register_device(dev: Box<dyn BlockDevice>) -> Option<usize> {
    REGISTRY.lock().register(dev)
}

pub fn get_static_device(id: usize) -> Option<&'static mut dyn BlockDevice> {
    REGISTRY.lock().get_static(id)
}

pub fn take_device(id: usize) -> Option<Box<dyn BlockDevice>> {
    // Redirigido o no utilizado; conservado por firma publica si se requiere
    let _ = id;
    None
}

pub fn with_device<F, R>(id: usize, f: F) -> R
where F: FnOnce(Option<&mut dyn BlockDevice>) -> R
{
    let dev_opt = REGISTRY.lock().get_static(id);
    let mut dev = dev_opt.map(|d| d as &mut dyn BlockDevice);
    f(dev.as_deref_mut())
}

pub fn device_count() -> usize {
    REGISTRY.lock().len()
}

pub fn flush_all() {
    let count = device_count();
    for id in 0..count {
        if let Some(dev) = get_static_device(id) {
            if dev.device_info().kind != DriveType::Atapi {
                let _ = dev.flush_cache();
            }
        }
    }
}
