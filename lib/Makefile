# lib/Makefile — libportix C runtime library for PORTIX ring-3
#
# Requires: x86_64-elf-gcc (cross-compiler for x86_64 ELF target)
# Build it with: ../scripts/ring3-toolchain.sh

CC      = x86_64-elf-gcc
AS      = x86_64-elf-as
AR      = x86_64-elf-ar
CFLAGS  = -ffreestanding -nostdlib -static -mno-red-zone -mno-mmx -mno-sse \
          -I include -O2 -Wall -Wextra
ASFLAGS =
ARFLAGS = rcs

SRCDIR  = src
INCDIR  = include
BUILDDIR = build

SRCS    = $(SRCDIR)/stdio.c $(SRCDIR)/stdlib.c $(SRCDIR)/string.c
OBJS    = $(SRCS:$(SRCDIR)/%.c=$(BUILDDIR)/%.o)
CRT0    = $(BUILDDIR)/crt0.o
LIB     = $(BUILDDIR)/libportix.a

.PHONY: all clean examples

all: $(LIB)

$(BUILDDIR):
	mkdir -p $(BUILDDIR)

$(BUILDDIR)/%.o: $(SRCDIR)/%.c $(INCDIR)/portix.h | $(BUILDDIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(CRT0): $(SRCDIR)/crt0.s | $(BUILDDIR)
	$(AS) $(ASFLAGS) $< -o $@

$(LIB): $(OBJS) $(CRT0)
	$(AR) $(ARFLAGS) $@ $(OBJS) $(CRT0)

examples: $(LIB)
	$(CC) $(CFLAGS) examples/hello.c -L $(BUILDDIR) -lportix -o $(BUILDDIR)/hello.elf

clean:
	rm -rf $(BUILDDIR)
