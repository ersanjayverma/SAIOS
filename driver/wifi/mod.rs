//! Wi-Fi subsystem — 802.11 MAC + WPA2 + driver abstraction.
//!
//! Supported drivers:
//!   iwlwifi  — Intel Wireless 7260/7265/8260/8265/AX200/AX201
//!
//! To connect to a WPA2 network:
//!   wifi scan               — list available networks
//!   wifi connect <ssid>     — connect to open network
//!   wifi connect <ssid> <password>  — connect to WPA2-PSK network
//!
//! 802.11 frames are injected into the existing TCP/IP stack via the
//! same TX_QUEUE / RX_QUEUE as the wired Ethernet drivers.

pub mod iwlwifi;
pub mod mac80211;
pub mod wpa2;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use mac80211::{BeaconInfo, MacAddr};
use spin::Mutex;

// -- WifiDriver trait -------------------------------------------------------

pub trait WifiDriver: Send + Sync {
    fn name(&self) -> &str;
    fn mac(&self) -> MacAddr;
    fn scan(&mut self) -> Vec<BeaconInfo>;
    fn connect(&mut self, ssid: &str, password: Option<&str>) -> Result<(), &'static str>;
    fn send(&mut self, frame: &[u8]);
    fn poll(&mut self) -> Vec<Vec<u8>>;
    fn is_connected(&self) -> bool;
    fn ssid(&self) -> Option<String>;
    fn signal_strength(&self) -> i8;
}

// -- Connection state -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum WifiState {
    Disconnected,
    Scanning,
    Authenticating,
    Associating,
    Connected { ssid: String, channel: u8, rssi: i8 },
}

pub static STATE: Mutex<WifiState> = Mutex::new(WifiState::Disconnected);
pub static AP_LIST: Mutex<Vec<BeaconInfo>> = Mutex::new(Vec::new());
static DRIVER: Mutex<Option<alloc::boxed::Box<dyn WifiDriver>>> = Mutex::new(None);

// -- Initialisation ---------------------------------------------------------

pub fn init() {
    if let Some(drv) = iwlwifi::IwlWifi::probe() {
        let mac = drv.mac();
        crate::println!("[wifi] {} active  MAC {}", drv.name(), mac.to_string());
        *DRIVER.lock() = Some(alloc::boxed::Box::new(drv));
        return;
    }
    crate::println!("[wifi] No supported Wi-Fi NIC found");
    crate::println!(
        "[wifi] Supported: Intel Wireless-N/AC/AX (iwlwifi) 7260/7265/8260/8265/AX200/AX201"
    );
}

pub fn present() -> bool {
    DRIVER.lock().is_some()
}

// -- Public operations -----------------------------------------------------

/// Perform an active scan and return discovered networks.
pub fn scan() -> Vec<BeaconInfo> {
    let mut drv = DRIVER.lock();
    if let Some(d) = drv.as_mut() {
        let networks = d.scan();
        let mut list = AP_LIST.lock();
        *list = networks.clone();
        return networks;
    }
    Vec::new()
}

/// Connect to an SSID, optionally with a WPA2-PSK password.
pub fn connect(ssid: &str, password: Option<&str>) -> Result<(), &'static str> {
    let mut drv = DRIVER.lock();
    if let Some(d) = drv.as_mut() {
        d.connect(ssid, password)?;
        *STATE.lock() = WifiState::Connected {
            ssid: String::from(ssid),
            channel: 0,
            rssi: -70,
        };
        Ok(())
    } else {
        Err("No Wi-Fi driver loaded")
    }
}

/// Disconnect from the current AP.
pub fn disconnect() {
    *STATE.lock() = WifiState::Disconnected;
}

/// Poll the Wi-Fi driver for received frames.
pub fn poll() {
    let mut drv = DRIVER.lock();
    if let Some(d) = drv.as_mut() {
        for frame in d.poll() {
            // Unwrap LLC/SNAP → push Ethernet frame into RX_QUEUE
            if frame.len() > 32 && frame[24..27] == [0xAA, 0xAA, 0x03] {
                let eth_payload = &frame[30..]; // skip 802.11 + LLC/SNAP
                crate::network_contract::NetworkContract::enqueue_rx(eth_payload.to_vec(), "wifi");
            }
        }
    }
}

// -- Shell interface --------------------------------------------------------

pub fn cmd_wifi(args: &str) {
    let mut parts = args.splitn(3, ' ');
    let sub = parts.next().unwrap_or("").trim();
    let arg1 = parts.next().unwrap_or("").trim();
    let arg2 = parts.next().unwrap_or("").trim();

    match sub {
        "scan" => {
            crate::println!("Scanning for Wi-Fi networks...");
            let nets = scan();
            if nets.is_empty() {
                crate::println!("No networks found.");
                if !present() {
                    crate::println!("(No Wi-Fi driver — check lspci)");
                }
                return;
            }
            crate::println!("{:<32} {:>4} {:>4}  Security", "SSID", "Ch", "RSSI");
            crate::println!("{}", "-".repeat(55));
            for n in &nets {
                let sec = if n.has_wpa3 {
                    "WPA3"
                } else if n.has_wpa2 {
                    "WPA2"
                } else {
                    "Open"
                };
                crate::println!(
                    "{:<32} {:>4} {:>3}dBm  {}",
                    if n.ssid.is_empty() {
                        "<hidden>"
                    } else {
                        &n.ssid
                    },
                    n.channel,
                    n.rssi,
                    sec
                );
            }
            crate::println!("{} network(s) found", nets.len());
        }
        "connect" if !arg1.is_empty() => {
            let pass = if arg2.is_empty() { None } else { Some(arg2) };
            crate::print!("Connecting to '{}'", arg1);
            if pass.is_some() {
                crate::print!(" (WPA2)");
            }
            crate::println!("...");
            match connect(arg1, pass) {
                Ok(()) => crate::println!("Connected to '{}'", arg1),
                Err(e) => crate::println!("Failed: {}", e),
            }
        }
        "disconnect" => {
            disconnect();
            crate::println!("Disconnected");
        }
        "status" => {
            let state = STATE.lock().clone();
            match state {
                WifiState::Connected {
                    ssid,
                    channel,
                    rssi,
                } => crate::println!("Connected: SSID={} ch={} rssi={}dBm", ssid, channel, rssi),
                WifiState::Disconnected => crate::println!("Not connected"),
                other => crate::println!("State: {:?}", other),
            }
            if let Some(d) = DRIVER.lock().as_ref() {
                crate::println!("Driver: {}  MAC: {}", d.name(), d.mac().to_string());
            }
        }
        "fw" => {
            // Load firmware: wifi fw iwlwifi-7260-17.ucode
            if arg1.is_empty() {
                crate::println!("usage: wifi fw <firmware-filename>");
                crate::println!("  Place the file in /lib/firmware/ first.");
                return;
            }
            // We need a mutable driver for this
            crate::println!(
                "Firmware loading: place {} in /lib/firmware/ and reboot.",
                arg1
            );
        }
        _ => {
            crate::println!("usage: wifi <scan|connect|disconnect|status|fw>");
            crate::println!("  scan                    — find networks");
            crate::println!("  connect <ssid> [pass]   — connect to WPA2/open");
            crate::println!("  disconnect              — disconnect");
            crate::println!("  status                  — show connection info");
            crate::println!("  fw <filename>           — load firmware file");
        }
    }
}
