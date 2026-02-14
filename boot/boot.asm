; boot/boot.asm
; Stage1 bootloader - Carga Stage2
; Ensamblar: nasm -f bin boot.asm -o boot.bin

BITS 16
org 0x7C00

STAGE2_SECTORS equ 64

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    ; Guardar unidad de arranque
    mov [boot_drive], dl

    ; Mensaje inicial
    mov si, msg_boot
    call print_string

    ; Resetear disco
    mov ah, 0x00
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    ; Preparar lectura de Stage2
    mov si, msg_loading
    call print_string

    mov word [current_lba], 1
    mov bx, 0x8000
    mov cx, STAGE2_SECTORS

.read_loop:
    push cx
    push bx

    ; Convertir LBA a CHS
    mov ax, [current_lba]
    call lba_to_chs

    ; Leer sector
    mov ah, 0x02
    mov al, 0x01
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    pop bx
    pop cx

    add bx, 512
    inc word [current_lba]
    loop .read_loop

    ; Stage2 cargado
    mov si, msg_jump
    call print_string

    ; Pasar unidad a Stage2 en DL
    mov dl, [boot_drive]
    
    ; Saltar a Stage2
    jmp 0x0000:0x8000

lba_to_chs:
    push ax
    push bx
    push dx

    xor dx, dx
    mov bx, 18
    div bx
    
    inc dx
    mov cl, dl
    
    xor dx, dx
    mov bx, 2
    div bx
    
    mov ch, al
    mov dh, dl

    pop dx
    pop bx
    pop ax
    ret

print_string:
    pusha
.loop:
    lodsb
    test al, al
    jz .done
    call print_char
    jmp .loop
.done:
    popa
    ret

print_char:
    pusha
    mov ah, 0x0E
    mov bh, 0
    mov bl, 7
    int 0x10
    popa
    ret

disk_error:
    mov si, msg_error
    call print_string
    cli
    hlt

msg_boot    db "PORTIX Boot", 13, 10, 0
msg_loading db "Loading Stage2", 0
msg_jump    db " OK!", 13, 10, 0
msg_error   db 13, 10, "Disk Error!", 13, 10, 0

boot_drive   db 0
current_lba  dw 0

times 510-($-$$) db 0
dw 0xAA55