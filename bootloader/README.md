# Bootloader for nRF

The bootloader uses `embassy-boot` to interact with the flash.

# Usage

Flash the bootloader

```
cargo flash --features embassy-nrf/nrf52840 --release --chip nRF52840_xxAA
```
