use core::cell::RefCell;

use embassy_boot::{FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::YieldingAsync;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use heapless::Vec;
use nrf_softdevice::{Flash, Softdevice};

const PAGE_SIZE: usize = 4096;
const CONTROL_START: u8 = 12;
const CONTROL_FINALIZE: u8 = 13;

// The FirmwareUpdate GATT service
#[nrf_softdevice::gatt_service(uuid = "00001000-b0cd-11ec-871f-d45ddf138840")]
pub struct FirmwareService {
    /// State control
    #[characteristic(uuid = "00001003-b0cd-11ec-871f-d45ddf138840", write)]
    control: u8,

    /// Firmware data to be written
    #[characteristic(uuid = "00001006-b0cd-11ec-871f-d45ddf138840", write)]
    firmware: Vec<u8, 64>,
}

struct DfuState {
    offset: usize,
    page_buf: [u8; PAGE_SIZE],
    buf_pos: usize,
    pending_finalize: bool,
}

pub struct BleDfu {
    flash: Mutex<NoopRawMutex, YieldingAsync<Flash>>,
    state: RefCell<DfuState>,
}

impl BleDfu {
    pub async fn new(sd: &'static Softdevice) -> Self {
        let flash = Flash::take(sd);
        let flash = Mutex::new(YieldingAsync::new(flash));

        {
            let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
            let mut magic = [0; 4];
            let mut updater = FirmwareUpdater::new(config, &mut magic);
            updater.mark_booted().await;
        }

        Self {
            flash,
            state: RefCell::new(DfuState {
                offset: 0,
                page_buf: [0; PAGE_SIZE],
                buf_pos: 0,
                pending_finalize: false,
            }),
        }
    }

    async fn flush_page(&self) {
        let (offset, buf, buf_pos) = {
            let state = self.state.borrow();
            if state.buf_pos == 0 {
                return;
            }
            (state.offset, state.page_buf, state.buf_pos)
        };

        let config = FirmwareUpdaterConfig::from_linkerfile(&self.flash, &self.flash);
        let mut magic = [0; 4];
        let mut updater = FirmwareUpdater::new(config, &mut magic);

        defmt::info!(
            "Writing page at offset {} ({} bytes buffered)",
            offset,
            buf_pos
        );
        updater.write_firmware(offset, &buf).await.unwrap();

        let mut state = self.state.borrow_mut();
        state.offset += PAGE_SIZE;
        state.buf_pos = 0;
        state.page_buf = [0; PAGE_SIZE];
    }

    pub async fn finalize_if_pending(&self) {
        if !self.state.borrow().pending_finalize {
            return;
        }

        defmt::info!("DFU finalize: flushing remaining data");
        self.flush_page().await;

        let config = FirmwareUpdaterConfig::from_linkerfile(&self.flash, &self.flash);
        let mut magic = [0; 4];
        let mut updater = FirmwareUpdater::new(config, &mut magic);
        updater.mark_updated().await.unwrap();

        defmt::info!("Firmware marked as updated, resetting...");
        cortex_m::peripheral::SCB::sys_reset();
    }

    pub async fn handle_event(&self, e: FirmwareServiceEvent) {
        match e {
            FirmwareServiceEvent::ControlWrite(control) => {
                defmt::info!("ControlWrite: {}", control);
                match control {
                    CONTROL_START => {
                        defmt::info!("DFU start");
                        let mut state = self.state.borrow_mut();
                        state.offset = 0;
                        state.buf_pos = 0;
                        state.page_buf = [0; PAGE_SIZE];
                        state.pending_finalize = false;
                    }
                    CONTROL_FINALIZE => {
                        defmt::info!("DFU finalize requested");
                        self.state.borrow_mut().pending_finalize = true;
                    }
                    _ => {
                        defmt::warn!("Unknown control value: {}", control);
                    }
                }
            }
            FirmwareServiceEvent::FirmwareWrite(data) => {
                let len = data.len();
                defmt::info!("FirmwareWrite: {} bytes", len);

                {
                    let mut state = self.state.borrow_mut();
                    let pos = state.buf_pos;
                    state.page_buf[pos..pos + len].copy_from_slice(&data);
                    state.buf_pos = pos + len;
                }

                if self.state.borrow().buf_pos >= PAGE_SIZE {
                    self.flush_page().await;
                }
            }
        }
    }
}
