use std::process::ExitCode;

mod calib;
mod crc;
mod env;
mod layout;
mod profile;
mod ubi;
mod ubifs;
mod xml;

use env::EnvSet;

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn arg_value(args: &[String], key: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| format!("missing required argument {key} <file>"))
}

fn load_env(args: &[String]) -> Result<EnvSet, String> {
    let primary = read_file(&arg_value(args, "--env")?)?;
    let secondary = match args.iter().position(|a| a == "--env2") {
        Some(i) => Some(read_file(args.get(i + 1).ok_or("--env2 requires a file")?)?),
        None => None,
    };
    let mut parts = vec![primary];
    if let Some(extra) = secondary {
        parts.push(extra);
    }
    let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
    env::load(&refs)
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(|s| s.as_str()) {
        Some("build") => {
            let model = profile::model(&arg_value(args, "--model")?)?;
            let output = arg_value(args, "-o")?;
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let extracted = (model.extract)(&env, &image)?;
            let sections = profile::sections(&extracted)?;
            let bin = layout::build(model.image_size, &sections)?;
            std::fs::write(&output, &bin)
                .map_err(|e| format!("cannot write {output}: {e}"))?;
            let mut report = format!(
                "factory image: {} ({} bytes) -> {}\n",
                model.name,
                bin.len(),
                output
            );
            for section in &sections {
                let spec = layout::field(section.name)?;
                report.push_str(&format!(
                    "  0x{:06x}  {:<22} size={:<8} crc=0x{:08x}\n",
                    spec.offset,
                    spec.name,
                    section.data.len(),
                    crc::zlib(&section.data)
                ));
            }
            Ok(report)
        }
        Some("dump") => {
            let model = profile::model(&arg_value(args, "--model")?)?;
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let extracted = (model.extract)(&env, &image)?;
            let mut report = format!("model: {}\n", model.name);
            report.push_str(&format!(
                "ubi: peb={} leb={} volumes={:?}\n",
                image.peb_size,
                image.leb_size,
                image.volumes.keys().collect::<Vec<_>>()
            ));
            let i = &extracted.identity;
            let mac = |m: &Option<[u8; 6]>| match m {
                Some(a) => format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    a[0], a[1], a[2], a[3], a[4], a[5]
                ),
                None => "-".into(),
            };
            report.push_str(&format!(
                "base_mac={} count={} epon_mac={}\n",
                mac(&i.base_mac),
                i.mac_count,
                mac(&i.epon_mac)
            ));
            for (name, value) in [("pon_sn", &i.pon_sn), ("device_sn", &i.device_sn)] {
                report.push_str(&format!("  {name}: {}\n", value.as_deref().unwrap_or("-")));
            }
            for (name, data) in [
                ("wifi0", &extracted.wifi0),
                ("wifi1", &extracted.wifi1),
                ("bosa", &extracted.bosa),
                ("bosa_olt", &extracted.bosa_olt),
                ("xtemp", &extracted.xtemp),
                ("fpga", &extracted.fpga),
            ] {
                report.push_str(&format!(
                    "  {}: {} bytes\n",
                    name,
                    data.as_ref().map_or(0, |d| d.len())
                ));
            }
            let sections = profile::sections(&extracted)?;
            report.push_str(&format!("sections: {}\n", sections.len()));
            Ok(report)
        }
        _ => Err("usage: rtl9617c-factory <build|dump> --model <tianyi-tyfza1-2p5g|youhua-pt1500-h10-ct-2p5g> --env <file> [--env2 <file>] --device <file> [-o <out.bin>]".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_error() {
        assert!(run(&[]).is_err());
    }
}
