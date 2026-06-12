#![no_std]
#![feature(alloc_error_handler)]
#![allow(static_mut_refs)]

extern crate alloc;

pub mod syscall;
pub mod io;
pub mod process;
pub mod fs;
pub mod allocator;
mod panic;
mod entry;

pub use io::{print, println};

#[global_allocator]
pub static ALLOCATOR: allocator::PortixAllocator = allocator::PortixAllocator;
