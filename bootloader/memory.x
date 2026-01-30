MEMORY
{
  FLASH                             : ORIGIN = 0x00000000, LENGTH = 8K
  BOOTLOADER_STATE                  : ORIGIN = 0x0 + 8K,   LENGTH = 4K
  ACTIVE                            : ORIGIN = 0x0 + 12K,  LENGTH = 504K
  DFU                               : ORIGIN = 0x0 + 516K, LENGTH = 508K
  RAM                         (rwx) : ORIGIN = 0x20000000, LENGTH = 32K
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE);

__bootloader_active_start = ORIGIN(ACTIVE);
__bootloader_active_end = ORIGIN(ACTIVE) + LENGTH(ACTIVE);

__bootloader_dfu_start = ORIGIN(DFU);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU);
