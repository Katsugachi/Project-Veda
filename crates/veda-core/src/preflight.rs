use crate::{context_budget, ModelQuant};
use serde::{Deserialize, Serialize};

const GIB: u64 = 1024 * 1024 * 1024;
pub const MINIMUM_FREE_DISK_BYTES: u64 = 10 * GIB;
pub const MINIMUM_MEMORY_BYTES: u64 = 6 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSnapshot {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub free_disk_bytes: u64,
    pub disk_kind: DiskKind,
    pub architecture: String,
    pub operating_system: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub free_disk_bytes: u64,
    pub disk_kind: DiskKind,
    pub architecture: String,
    pub operating_system: String,
    pub recommended_quant: ModelQuant,
    pub recommended_context: u32,
    pub hard_failures: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn evaluate_preflight(snapshot: HardwareSnapshot) -> PreflightReport {
    let mut hard_failures = Vec::new();
    let mut warnings = Vec::new();
    if snapshot.free_disk_bytes < MINIMUM_FREE_DISK_BYTES {
        hard_failures.push("At least 10 GiB of free disk space is required before setup.".into());
    }
    if snapshot.total_memory_bytes < MINIMUM_MEMORY_BYTES {
        hard_failures
            .push("At least 6 GiB of physical memory is required to run MiniCPM 5.".into());
    }
    if snapshot.disk_kind == DiskKind::Hdd {
        warnings.push(
            "An SSD is strongly recommended for model loading and documentation indexing.".into(),
        );
    } else if snapshot.disk_kind == DiskKind::Unknown {
        warnings.push("Veda could not verify that the selected data drive is an SSD.".into());
    }
    if snapshot.available_memory_bytes < 3 * GIB {
        warnings.push("Less than 3 GiB of memory is currently available; close other apps before loading the model.".into());
    }

    let recommended_quant = if snapshot.total_memory_bytes >= 12 * GIB {
        ModelQuant::Q8
    } else {
        ModelQuant::Q5
    };
    let budget = context_budget(snapshot.total_memory_bytes, recommended_quant);
    PreflightReport {
        total_memory_bytes: snapshot.total_memory_bytes,
        available_memory_bytes: snapshot.available_memory_bytes,
        free_disk_bytes: snapshot.free_disk_bytes,
        disk_kind: snapshot.disk_kind,
        architecture: snapshot.architecture,
        operating_system: snapshot.operating_system,
        recommended_quant,
        recommended_context: budget.context_tokens,
        hard_failures,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(memory: u64, disk: u64) -> HardwareSnapshot {
        HardwareSnapshot {
            total_memory_bytes: memory,
            available_memory_bytes: memory / 2,
            free_disk_bytes: disk,
            disk_kind: DiskKind::Ssd,
            architecture: "arm64".into(),
            operating_system: "test".into(),
        }
    }

    #[test]
    fn blocks_small_disk_and_memory() {
        let report = evaluate_preflight(snapshot(4 * GIB, 9 * GIB));
        assert_eq!(report.hard_failures.len(), 2);
    }

    #[test]
    fn q8_requires_twelve_gib() {
        assert_eq!(
            evaluate_preflight(snapshot(8 * GIB, 20 * GIB)).recommended_quant,
            ModelQuant::Q5
        );
        assert_eq!(
            evaluate_preflight(snapshot(16 * GIB, 20 * GIB)).recommended_quant,
            ModelQuant::Q8
        );
    }
}
