# SWD

cargo run --release

# DFU

cargo build --release

cargo objcopy --release -- -O binary firmware.bin

dfu-util -d c0de:cafe -a 0 -D firmware.bin