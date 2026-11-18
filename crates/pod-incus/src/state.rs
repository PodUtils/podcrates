//! The state of an instance (`shared/api/instance_state.go`), a profile
//! with its devices (`shared/api/profile.go`), and the count of the OOM
//! kills of an instance, which the state does not carry: the metrics of
//! incus (`GET /1.0/metrics`, `incus_memory_OOM_kills_total`) give it.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::client::Client;
use crate::envelope::nullable;
use crate::error::Result;
use crate::instances::Devices;
use crate::wire::escape;

/// The metric of the OOM kills of an instance.
pub const OOM_METRIC: &str = "incus_memory_OOM_kills_total";

/// The state of an instance (`GET /1.0/instances/NAME/state`). A value
/// that incusd cannot measure (a stopped instance, a pool with no usage)
/// is 0 or absent, as incusd gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InstanceState {
    /// `Running`, `Stopped`, `Frozen`, `Error`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: u16,
    /// The usage of each disk device, by name.
    #[serde(default, deserialize_with = "nullable")]
    pub disk: BTreeMap<String, DiskState>,
    #[serde(default)]
    pub memory: MemoryState,
    #[serde(default)]
    pub cpu: CpuState,
    /// The count of processes; -1 where incusd cannot count them.
    #[serde(default)]
    pub processes: i64,
    #[serde(default)]
    pub pid: i64,
}

/// The usage of a disk device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct DiskState {
    /// Bytes in use.
    #[serde(default)]
    pub usage: i64,
    /// Bytes of the device; 0 where it has no limit.
    #[serde(default)]
    pub total: i64,
}

/// The memory of an instance, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct MemoryState {
    #[serde(default)]
    pub usage: i64,
    #[serde(default)]
    pub usage_peak: i64,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub swap_usage: i64,
    #[serde(default)]
    pub swap_usage_peak: i64,
}

/// The CPU time of an instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct CpuState {
    /// Nanoseconds of CPU time.
    #[serde(default)]
    pub usage: i64,
    /// The CPU time that the instance may use in each second, in
    /// nanoseconds.
    #[serde(default)]
    pub allocated_time: i64,
}

/// A profile (`GET /1.0/profiles/NAME`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, deserialize_with = "nullable")]
    pub config: BTreeMap<String, String>,
    /// The devices: for the root disk, `root` with its `pool`.
    #[serde(default, deserialize_with = "nullable")]
    pub devices: Devices,
}

/// The value of `metric` for the instance `name` in the text of the
/// metrics (the exposition format of Prometheus); `None` where no sample
/// names it.
pub fn metric_of(text: &str, metric: &str, name: &str) -> Option<f64> {
    let label = format!("name=\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""));
    text.lines().filter(|l| !l.starts_with('#')).find_map(|line| {
        let rest = line.strip_prefix(metric)?;
        let (labels, value) = match rest.strip_prefix('{') {
            Some(rest) => rest.split_once('}')?,
            None => return None,
        };
        let names_it = labels.split(',').any(|pair| pair.trim() == label);
        // A value may have a time stamp after it.
        names_it.then(|| value.split_whitespace().next()?.parse().ok()).flatten()
    })
}

impl Client {
    /// `GET /1.0/instances/NAME/state`.
    pub async fn instance_state(&self, name: &str) -> Result<InstanceState> {
        self.get(&format!("/1.0/instances/{}/state", escape(name)), &[]).await
    }

    /// `GET /1.0/profiles/NAME`.
    pub async fn profile(&self, name: &str) -> Result<Profile> {
        self.get(&format!("/1.0/profiles/{}", escape(name)), &[]).await
    }

    /// The count of the OOM kills of the instance `name`, from
    /// `GET /1.0/metrics`; `None` where the metrics name no such sample
    /// (a stopped instance, an incusd with no metrics of memory).
    pub async fn oom_kills(&self, name: &str) -> Result<Option<u64>> {
        let reply = self.raw("/1.0/metrics", &[]).await?;
        let text = String::from_utf8_lossy(&reply.body);
        Ok(metric_of(&text, OOM_METRIC, name).filter(|v| *v >= 0.0).map(|v| v as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oom_kills_of_one_instance_in_the_metrics() {
        let text = "# HELP incus_memory_OOM_kills_total The number of out of memory kills.\n\
                    # TYPE incus_memory_OOM_kills_total counter\n\
                    incus_memory_OOM_kills_total{name=\"web\",project=\"default\",type=\"container\"} 0\n\
                    incus_memory_OOM_kills_total{name=\"job-7\",project=\"default\",type=\"container\"} 3 1700000000\n\
                    incus_memory_OOM_kills_total_x{name=\"other\"} 9\n";
        assert_eq!(metric_of(text, OOM_METRIC, "job-7"), Some(3.0));
        assert_eq!(metric_of(text, OOM_METRIC, "web"), Some(0.0));
        assert_eq!(metric_of(text, OOM_METRIC, "job"), None, "a name is matched whole");
        assert_eq!(metric_of(text, OOM_METRIC, "other"), None, "another metric with the same start");
        assert_eq!(metric_of("", OOM_METRIC, "web"), None);
    }

    #[test]
    fn a_state_reads_the_fields_of_instance_state_go() {
        let json = serde_json::json!({
            "status": "Running", "status_code": 103,
            "disk": {"root": {"usage": 1_048_576, "total": 0}},
            "memory": {"usage": 10, "usage_peak": 20, "total": 30, "swap_usage": 0, "swap_usage_peak": 0},
            "network": null, "pid": 4242, "processes": 7,
            "cpu": {"usage": 123_456_789, "allocated_time": 1_000_000_000}
        });
        let state: InstanceState = serde_json::from_value(json).unwrap();
        assert_eq!((state.status.as_str(), state.processes, state.pid), ("Running", 7, 4242));
        assert_eq!((state.memory.usage_peak, state.cpu.usage), (20, 123_456_789));
        assert_eq!(state.disk["root"].usage, 1_048_576);
        let stopped: InstanceState =
            serde_json::from_value(serde_json::json!({"status": "Stopped", "disk": null})).unwrap();
        assert!(stopped.disk.is_empty());
    }
}
