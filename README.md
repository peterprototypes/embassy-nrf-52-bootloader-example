# Example embassy-nrf-52-bootloader-example 

This example shows how to get several components working together on an nRF52840 chip.

- The binary blob SoftDevice 140 from Nordic for the bluetooth stack
- Embassy bootloader for swapping in new versions of the firmware
- A firmware app with usb DFU support and can be self updated via USB-DFU

All memory layouts, offsets and features are for nRF52840, but they can be adapted for any other device from the nRF52 family.

# SWD

When plugged in via SWD probe, three things need to happen:

1. Download and flash SoftDevice (you may need to add --allow-erase-all)
`probe-rs download --verify --binary-format hex --chip nRF52840_xxAA ~/Downloads/s140_nrf52_7.3.0_softdevice.hex`

2. Flash the bootloader
```bash
cd bootloader
cargo run --release --features softdevice
```

3. Initial flash of the application
```
cd ..
cargo run --release
```

# DFU

After all that is well and good further reprogramming of the app can happen only trough the usb port.

```
cargo build --release

cargo objcopy --release -- -O binary firmware.bin

dfu-util -d c0de:cafe -a 0 -D firmware.bin
```

It may take some time to see you app working again, after dfu-util uploads it and restarts the SoC, the bootloader needs to move it from the staging region in flash to the active one. It may take 20 seconds move 400kb on the flash chip.