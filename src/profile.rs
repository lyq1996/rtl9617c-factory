use crate::calib::{epon_llid_mac, flash_calibration, RADIO0_5G, RADIO1_2G};
use crate::env::{parse_mac, EnvSet};
use crate::layout::{encode_string, field, Section};
use crate::ubi::{Image as UbiImage, Volume};
use crate::ubifs;
use crate::xml::{field as xml_field, optional_field};
use std::collections::BTreeMap;

pub struct Identity {
    pub base_mac: Option<[u8; 6]>,
    pub mac_count: u32,
    pub epon_mac: Option<[u8; 6]>,
    pub pon_sn: Option<String>,
    pub device_sn: Option<String>,
}

pub struct Extracted {
    pub identity: Identity,
    pub wifi0: Option<Vec<u8>>,
    pub wifi1: Option<Vec<u8>>,
    pub bosa: Option<Vec<u8>>,
    pub bosa_olt: Option<Vec<u8>>,
    pub xtemp: Option<Vec<u8>>,
    pub fpga: Option<Vec<u8>>,
}

pub struct Model {
    pub name: &'static str,
    pub image_size: usize,
    pub extract: fn(&EnvSet, &UbiImage) -> Result<Extracted, String>,
}

pub const MODELS: &[Model] = &[
    Model { name: "tianyi-tyfza1-2p5g", image_size: 0x0080_0000, extract: extract_tianyi_tyfza1_2p5g },
    Model { name: "youhua-pt1500-h10-ct-2p5g", image_size: 0x0001_0000, extract: extract_youhua_pt1500_h10_ct_2p5g },
];

pub fn model(name: &str) -> Result<&'static Model, String> {
    MODELS
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("unknown model: {name}"))
}

fn flat_lebs(volume: &Volume) -> Vec<u8> {
    volume.lebs.concat()
}

fn extract_tianyi_tyfza1_2p5g(_env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
    let config = device.volume("ubi_Config")?;
    let lebs: BTreeMap<u32, Vec<u8>> = config
        .lebs
        .iter()
        .enumerate()
        .map(|(i, data)| (i as u32, data.clone()))
        .collect();
    let files = ubifs::extract(&lebs)?;
    let hs = read_text(&files, "config_hs.xml")?;
    let cs = read_text(&files, "config.xml")?;
    let base_mac = parse_mac(&xml_field(&hs, "ELAN_MAC_ADDR")?)?;
    let pon_sn = optional_field(&hs, "GPON_SN").filter(|v| !v.is_empty());
    let epon_mac = match epon_llid_mac(&cs) {
        Some(value) => Some(parse_mac(&value)?),
        None => None,
    };
    let fpga = match device.volumes.get("ubi_fpga") {
        Some(volume) => Some(flat_lebs(volume)),
        None => None,
    };
    Ok(Extracted {
        identity: Identity {
            base_mac: Some(base_mac),
            mac_count: 3,
            epon_mac,
            pon_sn,
            device_sn: None,
        },
        wifi0: Some(flash_calibration(&hs, &RADIO0_5G, base_mac)?),
        wifi1: Some(flash_calibration(&hs, &RADIO1_2G, base_mac)?),
        bosa: files.get("rtkbosa_k.bin").cloned(),
        bosa_olt: files.get("rtkoltbosa_k.bin").cloned(),
        xtemp: files.get("xtemp.cal").cloned(),
        fpga,
    })
}

fn extract_youhua_pt1500_h10_ct_2p5g(env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
    device.volume("ubi_Config")?;
    let required = ["kp_ucaBaseMacAddr", "kp_ulNumMacAddrs", "kp_szXponSN"];
    for key in required {
        if env.get(key).is_none() {
            return Err(format!("keyparam {key} is missing from the environment"));
        }
    }
    let base_mac = parse_mac(env.get("kp_ucaBaseMacAddr").unwrap())?;
    let mac_count = env
        .get("kp_ulNumMacAddrs")
        .unwrap()
        .trim()
        .parse::<u32>()
        .map_err(|e| format!("invalid kp_ulNumMacAddrs: {e}"))?;
    Ok(Extracted {
        identity: Identity {
            base_mac: Some(base_mac),
            mac_count,
            epon_mac: Some(base_mac),
            pon_sn: env.get("kp_szXponSN").map(|v| v.to_string()),
            device_sn: env.get("kp_szSerialNumber").map(|v| v.to_string()),
        },
        wifi0: None,
        wifi1: None,
        bosa: None,
        bosa_olt: None,
        xtemp: None,
        fpga: None,
    })
}

fn read_text(files: &ubifs::Files, name: &str) -> Result<String, String> {
    let data = files
        .get(name)
        .ok_or_else(|| format!("{name} is missing from ubi_Config"))?;
    String::from_utf8(data.clone()).map_err(|_| format!("{name} is not valid UTF-8"))
}

pub fn sections(extracted: &Extracted) -> Result<Vec<Section>, String> {
    let mut out = Vec::new();
    let i = &extracted.identity;
    if let Some(mac) = i.base_mac {
        let mut data = mac.to_vec();
        data.extend_from_slice(&i.mac_count.to_le_bytes());
        out.push(Section { name: "BASE_MAC", data });
    }
    if let Some(mac) = i.epon_mac {
        out.push(Section { name: "EPON_MAC", data: mac.to_vec() });
    }
    for (name, value) in [("PON_SN", &i.pon_sn), ("DEVICE_SN", &i.device_sn)] {
        if let Some(text) = value {
            let spec = field(name)?;
            out.push(Section {
                name,
                data: encode_string(text, spec.capacity as usize)?,
            });
        }
    }
    for (name, data) in [
        ("WIFI_CAL_RADIO0", &extracted.wifi0),
        ("WIFI_CAL_RADIO1", &extracted.wifi1),
        ("BOSA_CALIBRATION", &extracted.bosa),
        ("BOSA_OLT_CALIBRATION", &extracted.bosa_olt),
        ("XTEMP_CALIBRATION", &extracted.xtemp),
    ] {
        if let Some(payload) = data {
            out.push(Section {
                name,
                data: payload.clone(),
            });
        }
    }
    if let Some(fpga) = &extracted.fpga {
        out.push(Section { name: "FPGA_IMAGE", data: fpga.clone() });
    }
    Ok(out)
}
