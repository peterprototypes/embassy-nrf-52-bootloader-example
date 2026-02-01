#![no_std]
#![no_main]

use core::cell::RefCell;
use core::mem;

use defmt::unwrap;

use embassy_boot::BlockingFirmwareUpdater;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::Timer;

use embassy_usb::{Builder, Config};
use embassy_usb_dfu::consts::DfuAttributes;
use embassy_usb_dfu::{ResetImmediate, new_state, usb_dfu};

use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::interrupt::Priority;
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::usb::Driver;
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::wdt::{self, Watchdog, WatchdogHandle};
use embassy_nrf::{bind_interrupts, peripherals, usb};

use embassy_boot_nrf::FirmwareUpdaterConfig;

use nrf_softdevice::{Softdevice, raw};

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(
    struct Irqs {
        USBD => usb::InterruptHandler<peripherals::USBD>;
    }
);

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

// This is a randomly generated GUID to allow clients on Windows to find your device.
//
// N.B. update to a custom GUID for your own device!
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{EAA9A5DC-30BB-44BC-9232-606CDC875321}"];

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    defmt::info!("Working");

    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;

    let p = embassy_nrf::init(config);

    // softdevice init
    let config = nrf_softdevice::Config {
        clock: Some(raw::nrf_clock_lf_cfg_t {
            source: raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),
        conn_gap: Some(raw::ble_gap_conn_cfg_t {
            conn_count: 6,
            event_length: 24,
        }),
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 512 }),
        gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: raw::BLE_GATTS_ATTR_TAB_SIZE_DEFAULT,
        }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 3,
            central_role_count: 3,
            central_sec_count: 0,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            p_value: b"HelloRust" as *const u8 as _,
            current_len: 9,
            max_len: 9,
            write_perm: unsafe { mem::zeroed() },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
        }),
        ..Default::default()
    };

    let sd = Softdevice::enable(&config);
    spawner.spawn(unwrap!(softdevice_task(sd)));

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
    // Use software VBUS detect since SoftDevice controls the POWER_CLOCK peripheral
    let vbus_detect = SoftwareVbusDetect::new(true, true);
    let driver = Driver::new(p.USBD, Irqs, &vbus_detect);

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
    spawner.spawn(unwrap!(advertise_task(sd)));

    dev.run().await
}

#[embassy_executor::task]
async fn advertise_task(sd: &'static Softdevice) {
    use nrf_softdevice::ble::advertisement_builder::{
        Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload, ServiceList, ServiceUuid16,
    };
    use nrf_softdevice::ble::peripheral;

    let mut config = peripheral::Config::default();
    config.interval = 50;

    static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
        .services_16(ServiceList::Complete, &[ServiceUuid16::HEALTH_THERMOMETER]) // if there were a lot of these there may not be room for the full name
        .short_name("Hello")
        .build();

    // but we can put it in the scan data
    // so the full name is visible once connected
    static SCAN_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .full_name("Hello, Rust!")
        .build();

    let adv = peripheral::NonconnectableAdvertisement::ScannableUndirected {
        adv_data: &ADV_DATA,
        scan_data: &SCAN_DATA,
    };
    unwrap!(peripheral::advertise(sd, adv, &config).await);
}

#[embassy_executor::task]
async fn blink_and_pet(mut led: Output<'static>, mut wdt_handle: WatchdogHandle) -> ! {
    loop {
        wdt_handle.pet();
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();
        Timer::after_millis(100).await;
    }
}
