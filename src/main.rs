#![no_std]
#![no_main]

use core::cell::RefCell;

use defmt::unwrap;

use embassy_boot::BlockingFirmwareUpdater;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::Timer;

use embassy_usb::{Builder, Config};
use embassy_usb_dfu::consts::DfuAttributes;
use embassy_usb_dfu::{ResetImmediate, new_state, usb_dfu};

use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::usb::Driver;
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::wdt::{self, Watchdog, WatchdogHandle};
use embassy_nrf::{bind_interrupts, peripherals, usb};

use embassy_boot_nrf::FirmwareUpdaterConfig;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(
    struct Irqs {
        USBD => usb::InterruptHandler<peripherals::USBD>;
        CLOCK_POWER => usb::vbus_detect::InterruptHandler;
    }
);

// This is a randomly generated GUID to allow clients on Windows to find your device.
//
// N.B. update to a custom GUID for your own device!
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{EAA9A5DC-30BB-44BC-9232-606CDC875321}"];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    defmt::info!("Working");

    let p = embassy_nrf::init(Default::default());
    let led = Output::new(p.P0_13, Level::Low, OutputDrive::Standard);

    let wdt = p.WDT;
    let wdt_config = wdt::Config::try_new(&wdt).unwrap();
    let (_wdt, [wdt_handle]) = match Watchdog::try_new(wdt, wdt_config) {
        Ok(x) => x,
        Err(_) => {
            // Watchdog already active with the wrong number of handles, waiting for it to timeout...
            loop {
                cortex_m::asm::wfe();
            }
        }
    };

    // Create the driver, from the HAL.
    let driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-DFU Runtime example");
    config.serial_number = Some("1235678");

    let nvmc = Nvmc::new(p.NVMC);
    let nvmc = Mutex::new(RefCell::new(nvmc));

    let fw_config = FirmwareUpdaterConfig::from_linkerfile_blocking(&nvmc, &nvmc);
    let mut magic = [0; 4];
    let mut updater = BlockingFirmwareUpdater::new(fw_config, &mut magic);
    updater.mark_booted().expect("Failed to mark booted");

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 4096];

    let mut state = new_state(updater, DfuAttributes::CAN_DOWNLOAD, ResetImmediate);

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    // We add MSOS headers so that the device automatically gets assigned the WinUSB driver on Windows.
    // Otherwise users need to do this manually using a tool like Zadig.
    //
    // It seems these always need to be at added at the device level for this to work and for
    // composite devices they also need to be added on the function level (as shown later).
    //
    // builder.msos_descriptor(msos::windows_version::WIN8_1, 2);
    // builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    // builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
    //     "DeviceInterfaceGUIDs",
    //     msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    // ));

    usb_dfu::<_, _, _, _, 4096>(&mut builder, &mut state, |func| {
        // You likely don't have to add these function level headers if your USB device is not composite
        // (i.e. if your device does not expose another interface in addition to DFU)
        // func.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
        // func.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        //     "DeviceInterfaceGUIDs",
        //     msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
        // ));
    });

    let mut dev = builder.build();

    spawner.spawn(unwrap!(blink_and_pet(led, wdt_handle)));

    dev.run().await
}

#[embassy_executor::task]
async fn blink_and_pet(mut led: Output<'static>, mut wdt_handle: WatchdogHandle) -> ! {
    loop {
        wdt_handle.pet();
        led.set_high();
        Timer::after_millis(1000).await;
        led.set_low();
        Timer::after_millis(1000).await;
    }
}
