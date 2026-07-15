#!/bin/bash
# Compiles and runs hello_world with a virtio block device

cd /var/home/kreb/hermit_box/hermit-rs

IMG=/var/home/kreb/hermit_box/disks/raw.img
mkdir -p "$(dirname "$IMG")"
truncate -s 1G "$IMG"

if

    cargo build --target=x86_64-unknown-hermit \
        -Zbuild-std=std,panic_abort \
        --features fs \
        --profile dev \
        --package blk_fs
then

    qemu-system-x86_64 \
    -display none -serial stdio -kernel /var/home/kreb/hermit_box/hermit-loader-x86_64 \
    -initrd /var/home/kreb/hermit_box/hermit-rs/target/x86_64-unknown-hermit/debug/blk_fs \
    -cpu host -accel kvm -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -smp 1 -m 1024M -global virtio-mmio.force-legacy=off \
    -drive if=none,id=blk0,format=raw,file="$IMG" \
    -device virtio-blk-pci,drive=blk0,disable-legacy=on

fi
