use alloc::boxed::Box;
use crate::arch::Spinlock;
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

    fn get(&self, id: usize) -> Option<&dyn BlockDevice> {
        self.devices.get(id)?.as_ref().map(|b| &**b as &dyn BlockDevice)
    }

    fn get_mut(&mut self, id: usize) -> Option<&mut dyn BlockDevice> {
        self.devices.get_mut(id)?.as_mut().map(|b| &mut **b as &mut dyn BlockDevice)
    }

    fn len(&self) -> usize { self.count }
}

// SAFETY: El kernel PORTIX es single-threaded. DeviceRegistry solo contiene
// Box<dyn BlockDevice>, y BlockDevice solo se usa en contexto single-threaded.
unsafe impl Send for DeviceRegistry {}

static REGISTRY: Spinlock<DeviceRegistry> = Spinlock::new(DeviceRegistry::new());

pub fn register_device(dev: Box<dyn BlockDevice>) -> usize {
    REGISTRY.lock().register(dev)
}

pub fn get_device(id: usize) -> Option<&'static mut dyn BlockDevice> {
    let mut guard = REGISTRY.lock();
    match guard.get_mut(id) {
        Some(d) => {
            // SAFETY: El registro vive en un static (nunca se mueve).
            // El Spinlock protege el acceso concurrente.
            // El kernel es single-threaded: no hay carreras después del unlock.
            let extended: &'static mut dyn BlockDevice = unsafe {
                core::mem::transmute(d as &mut dyn BlockDevice)
            };
            Some(extended)
        }
        None => None,
    }
}

pub fn with_device<F, R>(id: usize, f: F) -> R
where F: FnOnce(Option<&mut dyn BlockDevice>) -> R
{
    let mut registry = REGISTRY.lock();
    let device = registry.get_mut(id);
    f(device)
}

pub fn device_count() -> usize {
    REGISTRY.lock().len()
}

pub fn flush_all() {
    let mut registry = REGISTRY.lock();
    let count = registry.len();
    for id in 0..count {
        if let Some(dev) = registry.get_mut(id) {
            if dev.device_info().kind != DriveType::Atapi {
                let _ = dev.flush_cache();
            }
        }
    }
}
