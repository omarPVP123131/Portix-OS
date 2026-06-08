#!/usr/bin/env python3
"""Generate a minimal ELF64 executable for the PORTIX ring-3 demo.
"""

import struct, sys, os

VADDR = 0x20000000
code = bytearray()
lea_positions = []

def emit_lea_rsi():
    global code, lea_positions
    lea_positions.append(len(code))
    code += b'\x48\x8d\x35' + struct.pack('<i', 0)

def emit_mov_eax(v):
    global code
    code += b'\xb8' + struct.pack('<I', v)

def emit_int80():
    global code
    code += b'\xcd\x80'

# Write "Hello\n"
emit_lea_rsi()
code += b'\xbf\x01\x00\x00\x00'
code += b'\xba\x06\x00\x00\x00'
emit_mov_eax(1)
emit_int80()

# Write "PID="
emit_lea_rsi()
code += b'\xbf\x01\x00\x00\x00'
code += b'\xba\x04\x00\x00\x00'
emit_mov_eax(1)
emit_int80()

# SYS_GETPID → r12
emit_mov_eax(2)
emit_int80()
code += b'\x49\x89\xc4'          # mov r12, rax

# Print PID digit
code += b'\x41\x8a\xc4'          # mov al, r12b
code += b'\x04\x30'              # add al, '0'
code += b'\x50'                  # push rax
code += b'\x48\x89\xe6'          # mov rsi, rsp
code += b'\xbf\x01\x00\x00\x00'
code += b'\xba\x01\x00\x00\x00'
emit_mov_eax(1)
emit_int80()
code += b'\x58'                  # pop rax

# newline
code += b'\x6a\x0a'
code += b'\x48\x89\xe6'
code += b'\xbf\x01\x00\x00\x00'
code += b'\xba\x01\x00\x00\x00'
emit_mov_eax(1)
emit_int80()
code += b'\x58'

# SYS_YIELD loop (20 iterations)
code += b'\x41\xbd\x14\x00\x00\x00'  # mov r13d, 20
loop_off = len(code)
code += b'\xb8\x03\x00\x00\x00'      # mov eax, 3
emit_int80()
code += b'\x41\xff\xcd'              # dec r13d
code += b'\x75\xf4'                  # jnz loop_start (rel8 = -12)

# Write "Done\n"
emit_lea_rsi()
code += b'\xbf\x01\x00\x00\x00'
code += b'\xba\x05\x00\x00\x00'
emit_mov_eax(1)
emit_int80()

# SYS_EXIT
code += b'\xb8\x00\x00\x00\x00'
code += b'\xbf\x00\x00\x00\x00'
emit_int80()

# Data
msg_hello = b'Hello\n'
msg_pid   = b'PID='
msg_done  = b'Done\n'
data = msg_hello + msg_pid + msg_done

# Fix RIP-relative offsets
code_offset = 64 + 56  # ELF header + program header
data_start = code_offset + len(code)

offsets = [data_start, data_start + len(msg_hello), data_start + len(msg_hello) + len(msg_pid)]
assert len(lea_positions) == len(offsets)

for pos, target in zip(lea_positions, offsets):
    insn_end = code_offset + pos + 7  # lea rsi, [rip+disp32] is 7 bytes
    struct.pack_into('<i', code, pos + 3, target - insn_end)

file_size = code_offset + len(code) + len(data)

elf = bytearray(64)
elf[0:4] = b'\x7fELF'
elf[4] = 2; elf[5] = 1; elf[6] = 1; elf[7] = 0
struct.pack_into('<H', elf, 16, 2)                    # ET_EXEC
struct.pack_into('<H', elf, 18, 0x3E)                 # x86-64
struct.pack_into('<I', elf, 20, 1)
struct.pack_into('<Q', elf, 24, VADDR + code_offset)  # entry
struct.pack_into('<Q', elf, 32, 64)                   # e_phoff
struct.pack_into('<Q', elf, 40, 0)
struct.pack_into('<I', elf, 48, 0)
struct.pack_into('<H', elf, 52, 64)
struct.pack_into('<H', elf, 54, 56)
struct.pack_into('<H', elf, 56, 1)
struct.pack_into('<H', elf, 58, 0)
struct.pack_into('<H', elf, 60, 0)
struct.pack_into('<H', elf, 62, 0)

ph = bytearray(56)
struct.pack_into('<I', ph, 0, 1)                      # PT_LOAD
struct.pack_into('<I', ph, 4, 7)                      # RWX
struct.pack_into('<Q', ph, 8, 0)
struct.pack_into('<Q', ph, 16, VADDR)
struct.pack_into('<Q', ph, 24, VADDR)
struct.pack_into('<Q', ph, 32, file_size)
struct.pack_into('<Q', ph, 40, file_size)
struct.pack_into('<Q', ph, 48, 0x1000)

result = bytes(elf) + bytes(ph) + bytes(code) + data

out_path = sys.argv[1] if len(sys.argv) > 1 else 'build/hello.elf'
os.makedirs(os.path.dirname(out_path) or '.', exist_ok=True)
with open(out_path, 'wb') as f:
    f.write(result)

print(f"[OK]    hello.elf — {len(result)} bytes, entry=0x{VADDR + code_offset:x}")
