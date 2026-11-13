pub struct Field {
    pub name: &'static str,
    pub offset: u32,
    pub capacity: u32,
}

pub const FIELDS: &[Field] = &[
    Field { name: "BASE_MAC", offset: 0x1000, capacity: 0x10 },
    Field { name: "EPON_MAC", offset: 0x1010, capacity: 0x06 },
    Field { name: "PON_SN", offset: 0x1020, capacity: 0x10 },
    Field { name: "DEVICE_SN", offset: 0x1030, capacity: 0x20 },
    Field { name: "WIFI_CAL_RADIO0", offset: 0x2000, capacity: 0x1000 },
    Field { name: "WIFI_CAL_RADIO1", offset: 0x3000, capacity: 0x1000 },
    Field { name: "BOSA_CALIBRATION", offset: 0x4000, capacity: 0x0800 },
    Field { name: "BOSA_OLT_CALIBRATION", offset: 0x4800, capacity: 0x0400 },
    Field { name: "XTEMP_CALIBRATION", offset: 0x5000, capacity: 0x1000 },
    Field { name: "FPGA_IMAGE", offset: 0x10000, capacity: 0x7E0000 },
];

pub fn field(name: &str) -> Result<&'static Field, String> {
    FIELDS
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("unknown factory field {name}"))
}

pub fn encode_string(value: &str, capacity: usize) -> Result<Vec<u8>, String> {
    let mut out = value.as_bytes().to_vec();
    out.push(0);
    if out.len() > capacity {
        return Err(format!(
            "string field does not fit: {} bytes > {} capacity",
            out.len(),
            capacity
        ));
    }
    Ok(out)
}

pub struct Section {
    pub name: &'static str,
    pub data: Vec<u8>,
}

pub fn build(image_size: usize, sections: &[Section]) -> Result<Vec<u8>, String> {
    let mut image = vec![0xffu8; image_size];
    let mut written: Vec<&'static str> = Vec::new();
    for section in sections {
        let spec = field(section.name)?;
        if written.contains(&spec.name) {
            return Err(format!("duplicate section {}", spec.name));
        }
        if section.data.len() > spec.capacity as usize {
            return Err(format!(
                "section {} exceeds capacity: {} > {}",
                spec.name,
                section.data.len(),
                spec.capacity
            ));
        }
        let offset = spec.offset as usize;
        let end = offset + section.data.len();
        if end > image_size {
            return Err(format!(
                "section {} exceeds image size: 0x{end:x} > 0x{image_size:x}",
                spec.name
            ));
        }
        image[offset..end].copy_from_slice(&section.data);
        written.push(spec.name);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_sections_at_fixed_offsets() {
        let image = build(
            0x2000,
            &[
                Section { name: "EPON_MAC", data: vec![1, 2, 3, 4, 5, 6] },
                Section { name: "PON_SN", data: b"TYZDD0046AB9\0".to_vec() },
            ],
        )
        .unwrap();
        assert_eq!(image.len(), 0x2000);
        assert_eq!(image[0x1010..0x1010 + 6], [1, 2, 3, 4, 5, 6]);
        assert_eq!(&image[0x1020..0x1020 + 13], b"TYZDD0046AB9\0");
        assert_eq!(image[0x102D], 0xff);
        assert_eq!(image[0], 0xff);
    }

    #[test]
    fn rejects_oversize() {
        assert!(build(
            0x2000,
            &[Section { name: "PON_SN", data: vec![0x41; 32] }]
        )
        .is_err());
    }

    #[test]
    fn rejects_section_past_image_size() {
        assert!(build(
            0x1000,
            &[Section { name: "BASE_MAC", data: vec![0x41; 16] }]
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_field() {
        assert!(build(
            0x2000,
            &[Section { name: "UNKNOWN", data: vec![0x41; 4] }]
        )
        .is_err());
    }
}
