use core::alloc::{GlobalAlloc, Layout};
use crate::syscall;

pub struct PortixAllocator;

unsafe impl GlobalAlloc for PortixAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        let current = syscall::sys_brk(0) as usize;
        let aligned = (current + align - 1) & !(align - 1);
        let new_brk = syscall::sys_brk(aligned + size) as usize;
        if new_brk < aligned + size {
            return core::ptr::null_mut();
        }
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    syscall::sys_write(2, "alloc error\n".as_ptr(), 12);
    syscall::sys_exit(1)
}
