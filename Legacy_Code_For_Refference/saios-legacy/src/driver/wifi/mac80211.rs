//! IEEE 802.11 MAC layer — frame encoding/decoding, SSID management.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// -- Frame Control field ---------------------------------------------------

pub const FTYPE_MGMT: u8 = 0x00;
pub const FTYPE_CTRL: u8 = 0x01;
pub const FTYPE_DATA: u8 = 0x02;

pub const FSUB_ASSOC_REQ: u8 = 0x00;
pub const FSUB_ASSOC_RESP: u8 = 0x01;
pub const FSUB_PROBE_REQ: u8 = 0x04;
pub const FSUB_PROBE_RESP: u8 = 0x05;
pub const FSUB_BEACON: u8 = 0x08;
pub const FSUB_AUTH: u8 = 0x0B;
pub const FSUB_DEAUTH: u8 = 0x0C;
pub const FSUB_DATA: u8 = 0x00;

#[derive(Debug, Clone)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: Self = MacAddr([0xFF; 6]);
    pub const ZERO: Self = MacAddr([0x00; 6]);

    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        alloc::format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3],
            self.0[4],
            self.0[5]
        )
    }
}

// -- 802.11 Frame ----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Dot11Frame {
    pub frame_ctrl: u16, // type + subtype + flags
    pub duration: u16,
    pub addr1: MacAddr, // destination
    pub addr2: MacAddr, // source / BSSID
    pub addr3: MacAddr, // BSSID / source / destination
    pub seq_ctrl: u16,
    pub payload: Vec<u8>,
}

impl Dot11Frame {
    pub fn frame_type(&self) -> u8 {
        ((self.frame_ctrl >> 2) & 0x3) as u8
    }
    pub fn frame_subtype(&self) -> u8 {
        ((self.frame_ctrl >> 4) & 0xF) as u8
    }
    pub fn to_ds(&self) -> bool {
        self.frame_ctrl & (1 << 8) != 0
    }
    #[allow(clippy::wrong_self_convention)]
    pub fn from_ds(&self) -> bool {
        self.frame_ctrl & (1 << 9) != 0
    }
    pub fn has_wep(&self) -> bool {
        self.frame_ctrl & (1 << 14) != 0
    }

    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 24 {
            return None;
        }
        let fc = u16::from_le_bytes([raw[0], raw[1]]);
        Some(Self {
            frame_ctrl: fc,
            duration: u16::from_le_bytes([raw[2], raw[3]]),
            addr1: MacAddr(raw[4..10].try_into().ok()?),
            addr2: MacAddr(raw[10..16].try_into().ok()?),
            addr3: MacAddr(raw[16..22].try_into().ok()?),
            seq_ctrl: u16::from_le_bytes([raw[22], raw[23]]),
            payload: raw[24..].to_vec(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(24 + self.payload.len());
        out.extend_from_slice(&self.frame_ctrl.to_le_bytes());
        out.extend_from_slice(&self.duration.to_le_bytes());
        out.extend_from_slice(&self.addr1.0);
        out.extend_from_slice(&self.addr2.0);
        out.extend_from_slice(&self.addr3.0);
        out.extend_from_slice(&self.seq_ctrl.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

// -- Information Elements (tags in management frames) ----------------------

pub fn ie_get(payload: &[u8], id: u8) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 2 <= payload.len() {
        let ie_id = payload[pos];
        let ie_len = payload[pos + 1] as usize;
        if pos + 2 + ie_len > payload.len() {
            break;
        }
        if ie_id == id {
            return Some(&payload[pos + 2..pos + 2 + ie_len]);
        }
        pos += 2 + ie_len;
    }
    None
}

/// Append one IE (tag-length-value) to a management frame body.
pub fn ie_append(out: &mut Vec<u8>, id: u8, data: &[u8]) {
    out.push(id);
    out.push(data.len() as u8);
    out.extend_from_slice(data);
}

// -- Beacon frame parsing --------------------------------------------------

#[derive(Debug, Clone)]
pub struct BeaconInfo {
    pub bssid: MacAddr,
    pub ssid: String,
    pub channel: u8,
    pub rssi: i8, // set by driver
    pub has_wpa2: bool,
    pub has_wpa3: bool,
    pub has_open: bool,
    pub beacon_interval: u16,
    pub capabilities: u16,
}

pub fn parse_beacon(frame: &Dot11Frame, rssi: i8) -> Option<BeaconInfo> {
    if frame.frame_subtype() != FSUB_BEACON {
        return None;
    }
    if frame.payload.len() < 12 {
        return None;
    }

    let beacon_interval = u16::from_le_bytes([frame.payload[8], frame.payload[9]]);
    let capabilities = u16::from_le_bytes([frame.payload[10], frame.payload[11]]);
    let ies = &frame.payload[12..];

    let ssid = ie_get(ies, 0)
        .and_then(|b| String::from_utf8(b.to_vec()).ok())
        .unwrap_or_default();
    let channel = ie_get(ies, 3).and_then(|b| b.first().copied()).unwrap_or(0);

    // RSN IE (id=48) = WPA2/WPA3
    let rsn = ie_get(ies, 48);
    let wpa_vendor = ie_get(ies, 221); // vendor-specific OUI 00:50:F2:01 = WPA
    let has_wpa2 = rsn.is_some();
    let has_wpa3 = rsn
        .map(|r| r.len() >= 2 && r[0] == 1 && r[1] == 0)
        .unwrap_or(false);
    let has_open = !has_wpa2 && wpa_vendor.is_none();

    Some(BeaconInfo {
        bssid: frame.addr3.clone(),
        ssid,
        channel,
        rssi,
        has_wpa2,
        has_wpa3,
        has_open,
        beacon_interval,
        capabilities,
    })
}

// -- Probe request (active scan) -------------------------------------------

pub fn build_probe_request(src_mac: &MacAddr, ssid: Option<&str>) -> Vec<u8> {
    let fc: u16 = ((FTYPE_MGMT as u16) << 2) | ((FSUB_PROBE_REQ as u16) << 4);
    let mut payload = Vec::new();

    // SSID IE
    let ssid_bytes = ssid.map(|s| s.as_bytes()).unwrap_or(b"");
    ie_append(&mut payload, 0, ssid_bytes);

    // Supported rates IE
    ie_append(
        &mut payload,
        1,
        &[0x82, 0x84, 0x8B, 0x96, 0x24, 0x30, 0x48, 0x6C],
    );

    // Extended supported rates
    ie_append(&mut payload, 50, &[0x0C, 0x12, 0x18, 0x60]);

    Dot11Frame {
        frame_ctrl: fc,
        duration: 0,
        addr1: MacAddr::BROADCAST,
        addr2: src_mac.clone(),
        addr3: MacAddr::BROADCAST,
        seq_ctrl: 0,
        payload,
    }
    .encode()
}

// -- Data frame (LLC/SNAP encapsulation) -----------------------------------

/// Wrap an Ethernet payload in an 802.11 data frame with LLC/SNAP header.
pub fn build_data_frame(
    src: &MacAddr,
    dst: &MacAddr,
    bssid: &MacAddr,
    ethertype: u16,
    payload: &[u8],
) -> Vec<u8> {
    // FC: data, to_ds=1
    let fc: u16 = ((FTYPE_DATA as u16) << 2) | ((FSUB_DATA as u16) << 4) | (1 << 8);

    // LLC/SNAP header: AA AA 03 00 00 00 <ethertype>
    let mut body = alloc::vec![0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00];
    body.extend_from_slice(&ethertype.to_be_bytes());
    body.extend_from_slice(payload);

    Dot11Frame {
        frame_ctrl: fc,
        duration: 0,
        addr1: bssid.clone(), // AP
        addr2: src.clone(),
        addr3: dst.clone(),
        seq_ctrl: 0,
        payload: body,
    }
    .encode()
}
