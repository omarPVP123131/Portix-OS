pub mod halt;
pub mod idt;
pub mod hardware;
pub mod isr_handlers;
pub mod ring3;
pub mod spinlock;

// Re-export spinlock for convenient access
pub use spinlock::Spinlock;