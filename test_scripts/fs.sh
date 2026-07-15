#!/bin/bash

cd /var/home/kreb/hermit_box/hermit-rs

FS_DIR=/var/home/kreb/hermit_box/disks/fs_dir
rm -rf "$FS_DIR"
mkdir -p "$FS_DIR"

if

    cargo build --target=x86_64-unknown-hermit \
        -Zbuild-std=std,panic_abort \
        --features fs \
        --profile dev \
        --package blk_fs
then
    rm -f /tmp/vhostqemu
    virtiofsd \
    --socket-path=/tmp/vhostqemu \
    --shared-dir "$FS_DIR" \
    --announce-submounts \
    --sandbox none \
    --seccomp none \
    --inode-file-handles=never &

    VIRTIOFSD_PID=$!

    qemu-system-x86_64 \
    -display none -serial stdio -kernel /var/home/kreb/hermit_box/hermit-loader-x86_64 \
    -initrd /var/home/kreb/hermit_box/hermit-rs/target/x86_64-unknown-hermit/debug/blk_fs \
    -cpu host -accel kvm -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -smp 1 -m 1024M -global virtio-mmio.force-legacy=off \
    -chardev socket,id=char0,path=/tmp/vhostqemu \
    -device vhost-user-fs-pci,queue-size=1024,packed=on,chardev=char0,tag=root \
    -object memory-backend-file,id=mem,size=1024M,mem-path=/dev/shm,share=on \
    -numa node,memdev=mem

    kill "$VIRTIOFSD_PID"
fi
