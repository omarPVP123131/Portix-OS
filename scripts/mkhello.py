#!/usr/bin/env python3
"""Generate a minimal ELF64 executable for the PORTIX ring-3 demo.

The program writes "Hello from Ring 3!\n" via SYS_WRITE (int 0x80, rax=1),
then exits via SYS_EXIT (int 0x80, rax=0).

Output: build/hello.elf  (or first CLI argument)
"""

import struct, sys, os

VADDR = 0x20000000

code = bytearray()
rip_off = 29
code += b'\x48\x8d\x35' + struct.pack('<i', rip_off)  # lea rsi, [rip + 29]
code += b'\xbf\x01\x00\x00\x00'                         # mov edi, 1
code += b'\xba\x13\x00\x00\x00'                         # mov edx, 19
code += b'\xb8\x01\x00\x00\x00'                         # mov eax, 1 (SYS_WRITE)
code += b'\xcd\x80'                                     # int 0x80
code += b'\xb8\x00\x00\x00\x00'                         # mov eax, 0 (SYS_EXIT)
code += b'\xbf\x00\x00\x00\x00'                         # mov edi, 0
code += b'\xcd\x80'                                     # int 0x80

msg = b'Hello from Ring 3!\n'

code_offset = 64 + 56  # ELF header + program header
file_size = code_offset + len(code) + len(msg)

elf = bytearray(64)
elf[0:4] = b'\x7fELF'
elf[4] = 2    # 64-bit
elf[5] = 1    # little-endian
elf[6] = 1    # version
elf[7] = 0    # OS/ABI
struct.pack_into('<H', elf, 16, 2)                      # ET_EXEC
struct.pack_into('<H', elf, 18, 0x3E)                   # x86-64
struct.pack_into('<I', elf, 20, 1)                      # version
struct.pack_into('<Q', elf, 24, VADDR + code_offset)    # entry
struct.pack_into('<Q', elf, 32, 64)                     # e_phoff
struct.pack_into('<Q', elf, 40, 0)                      # e_shoff
struct.pack_into('<I', elf, 48, 0)                      # e_flags
struct.pack_into('<H', elf, 52, 64)                     # e_ehsize
struct.pack_into('<H', elf, 54, 56)                     # e_phentsize
struct.pack_into('<H', elf, 56, 1)                      # e_phnum
struct.pack_into('<H', elf, 58, 0)                      # e_shentsize
struct.pack_into('<H', elf, 60, 0)                      # e_shnum
struct.pack_into('<H', elf, 62, 0)                      # e_shstrndx

ph = bytearray(56)
struct.pack_into('<I', ph, 0, 1)                        # PT_LOAD
struct.pack_into('<I', ph, 4, 7)                        # RWX
struct.pack_into('<Q', ph, 8, 0)                        # p_offset
struct.pack_into('<Q', ph, 16, VADDR)                   # p_vaddr
struct.pack_into('<Q', ph, 24, VADDR)                   # p_paddr
struct.pack_into('<Q', ph, 32, file_size)               # p_filesz
struct.pack_into('<Q', ph, 40, file_size)               # p_memsz
struct.pack_into('<Q', ph, 48, 0x1000)                  # p_align

result = bytes(elf) + bytes(ph) + bytes(code) + msg

out_path = sys.argv[1] if len(sys.argv) > 1 else 'build/hello.elf'
os.makedirs(os.path.dirname(out_path) or '.', exist_ok=True)
with open(out_path, 'wb') as f:
    f.write(result)

print(f"[OK]    hello.elf — {len(result)} bytes, entry=0x{VADDR + code_offset:x}")
