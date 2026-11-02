use crate::syscall;

pub fn init_module(image: &[u8], params: &str) -> i64 {
    let mut param_buf = [0u8; 128];
    let bytes = params.as_bytes();
    let len = bytes.len().min(127);
    param_buf[..len].copy_from_slice(&bytes[..len]);
    param_buf[len] = 0;
    syscall::sys_init_module(image.as_ptr(), image.len(), param_buf.as_ptr())
}

pub fn delete_module(name: &str, flags: i32) -> i64 {
    let mut name_buf = [0u8; 64];
    let bytes = name.as_bytes();
    let len = bytes.len().min(63);
    name_buf[..len].copy_from_slice(&bytes[..len]);
    name_buf[len] = 0;
    syscall::sys_delete_module(name_buf.as_ptr(), flags)
}

pub fn query_module(buf: &mut [u8]) -> i64 {
    syscall::sys_query_module(buf.as_mut_ptr(), buf.len())
}
