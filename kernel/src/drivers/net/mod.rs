// kernel/src/drivers/net/mod.rs — Network Device Drivers
pub mod e1000;
pub mod wifi_80211;

use crate::drivers::bus::pci::PciBus;

pub fn init() {
    let bus = PciBus::scan();
    for i in 0..bus.count {
        let dev = &bus.devices[i];
        // Intel e1000 Vendor ID 0x8086, Device ID 0x100E (82540EM) or similar
        if dev.vendor_id == 0x8086 && (dev.device_id == 0x100E || dev.device_id == 0x1004 || dev.device_id == 0x100F || dev.device_id == 0x10D3) {
            if e1000::init(dev) {
                break;
            }
        }
    }
}
