use super::{error, Result};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    Include,
    Preserve,
    Block,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub disposition: Disposition,
    pub reason: String,
}

pub fn safe_relative(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 240 || name.contains('\\') || name.contains(':') {
        return Err(error("PATH", "Invalid package-relative path"));
    }
    for component in name.split('/') {
        // Restrict names so NTFS ADS, device names and case/normalization aliases cannot escape checks.
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            || !component
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || matches!(
                component
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "con"
                    | "prn"
                    | "aux"
                    | "nul"
                    | "com1"
                    | "com2"
                    | "com3"
                    | "com4"
                    | "com5"
                    | "com6"
                    | "com7"
                    | "com8"
                    | "com9"
                    | "lpt1"
                    | "lpt2"
                    | "lpt3"
                    | "lpt4"
                    | "lpt5"
                    | "lpt6"
                    | "lpt7"
                    | "lpt8"
                    | "lpt9"
            )
        {
            return Err(error("PATH", "Unsafe package path component"));
        }
    }
    Ok(name.to_ascii_lowercase())
}

pub fn partition_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(error("PARTITION", "Invalid SIN partition name"));
    }
    Ok(())
}

pub fn decide(
    relative: &str,
    partition: Option<&str>,
    noerase: &BTreeSet<String>,
) -> Result<Decision> {
    let relative = safe_relative(relative)?;
    let base = relative.rsplit('/').next().unwrap();
    let result = |disposition, reason: &str| {
        Ok(Decision {
            disposition,
            reason: reason.into(),
        })
    };
    if relative.ends_with(".ta") {
        return result(Disposition::Preserve, "input-ta-excluded");
    }
    if noerase.contains(&relative) {
        return result(Disposition::Preserve, "update-xml-noerase");
    }
    let Some(partition) = partition else {
        return result(Disposition::Block, "unclassified-input");
    };
    partition_name(partition)?;
    let external = base
        .split("_x-flash")
        .next()
        .unwrap()
        .strip_suffix(".sin")
        .unwrap_or(base.split("_x-flash").next().unwrap());
    let external = external.strip_suffix("_other").unwrap_or(external);
    if external != partition {
        return Err(error(
            "TARGET_MISMATCH",
            "SIN filename and internal partition disagree",
        ));
    }
    if partition.starts_with("modem") || partition == "dsp" || partition.starts_with("dsp_") {
        return result(Disposition::Preserve, "volte-modem-dsp-preservation");
    }
    if matches!(
        partition,
        "userdata" | "metadata" | "persist" | "persistent" | "misc" | "frp" | "keystore" | "cache"
    ) {
        return result(
            Disposition::Preserve,
            "data-encryption-device-state-preservation",
        );
    }
    if relative.starts_with("partition/") || partition.starts_with("partitionimage") {
        return result(Disposition::Block, "partition-layout-not-validated");
    }
    if matches!(
        partition,
        "boot"
            | "init_boot"
            | "vendor_boot"
            | "dtbo"
            | "vbmeta"
            | "vbmeta_system"
            | "vbmeta_vendor"
            | "system"
            | "system_ext"
            | "vendor"
            | "product"
            | "odm"
            | "super"
            | "recovery"
            | "bluetooth"
            | "bootloader"
            | "rdimage"
    ) {
        return result(Disposition::Include, "candidate-requires-device-profile");
    }
    result(Disposition::Block, "unknown-partition")
}
