//! What the monitor records every sample interval, and how it is written as CSV.

use super::probe::Fds;

/// One row of `samples.csv`: the engine process, its snapshot and the peers' totals.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sample {
    pub(crate) t_s: u64,
    pub(crate) unix_ms: u64,
    pub(crate) incarnation: u32,
    pub(crate) up: bool,
    pub(crate) pid: u32,
    /// Memory the process is responsible for (see `probe::footprint_kib`); 0 when unknown.
    pub(crate) footprint_kib: u64,
    /// Resident size: the secondary series.
    pub(crate) rss_kib: u64,
    /// CPU use since the previous sample, percent times ten.
    pub(crate) cpu_tenths: u64,
    pub(crate) fds: Fds,
    pub(crate) store_bytes: u64,
    pub(crate) state_bytes: u64,
    pub(crate) logs_bytes: u64,
    pub(crate) log_files: u64,
    /// Events in the log ring; 0 when not read at this sample.
    pub(crate) ring_events: u64,
    /// Round trip of the snapshot request, milliseconds.
    pub(crate) rtt_ms: u64,
    pub(crate) snapshots: u64,
    pub(crate) gap_max_ms: u64,
    pub(crate) age_p50_ms: u64,
    pub(crate) age_p99_ms: u64,
    pub(crate) node_state: String,
    pub(crate) store_state: String,
    pub(crate) running: u64,
    pub(crate) retrying: u64,
    pub(crate) degraded: u64,
    pub(crate) stopped: u64,
    pub(crate) endpoints_up: u64,
    pub(crate) endpoints_total: u64,
    pub(crate) alerts_warning: u64,
    pub(crate) alerts_critical: u64,
    pub(crate) messages_stored: u64,
    pub(crate) deliveries_unresolved: u64,
    pub(crate) deliveries_queued: u64,
    pub(crate) acked: u64,
    pub(crate) received: u64,
    pub(crate) bytes_sent: u64,
    pub(crate) bytes_received: u64,
    pub(crate) e2e_count: u64,
    pub(crate) e2e_p50_ms: u64,
    pub(crate) e2e_p99_ms: u64,
    pub(crate) episodes_active: u32,
}

impl Sample {
    /// The series memory growth is judged on: the footprint, or the resident size where no
    /// footprint exists.
    pub(crate) fn memory_kib(&self) -> u64 {
        if self.footprint_kib > 0 {
            self.footprint_kib
        } else {
            self.rss_kib
        }
    }

    pub(crate) const HEADER: &'static str = "t_s,unix_ms,incarnation,up,pid,footprint_mib,rss_mib,cpu_pct,fds,fd_sockets,fd_files,\
store_mib,state_mib,logs_mib,log_files,ring_events,snapshot_rtt_ms,snapshots,snapshot_gap_max_ms,\
snapshot_age_p50_ms,snapshot_age_p99_ms,node_state,store_state,ch_running,ch_retrying,ch_degraded,\
ch_stopped,endpoints_up,endpoints_total,alerts_warning,alerts_critical,messages_stored,\
deliveries_unresolved,deliveries_queued,peer_acked,peer_received,bridge_bytes_sent,\
bridge_bytes_received,e2e_count,e2e_p50_ms,e2e_p99_ms,episodes_active";

    pub(crate) fn csv(&self) -> String {
        let mib = |bytes: u64| format!("{}.{}", bytes >> 20, ((bytes & 0xF_FFFF) * 10) >> 20);
        let tenths = |n: u64| format!("{}.{}", n / 10, n % 10);
        [
            self.t_s.to_string(),
            self.unix_ms.to_string(),
            self.incarnation.to_string(),
            u8::from(self.up).to_string(),
            self.pid.to_string(),
            mib(self.footprint_kib << 10),
            mib(self.rss_kib << 10),
            tenths(self.cpu_tenths),
            self.fds.total.to_string(),
            self.fds.sockets.to_string(),
            self.fds.files.to_string(),
            mib(self.store_bytes),
            mib(self.state_bytes),
            mib(self.logs_bytes),
            self.log_files.to_string(),
            self.ring_events.to_string(),
            self.rtt_ms.to_string(),
            self.snapshots.to_string(),
            self.gap_max_ms.to_string(),
            self.age_p50_ms.to_string(),
            self.age_p99_ms.to_string(),
            self.node_state.clone(),
            self.store_state.clone(),
            self.running.to_string(),
            self.retrying.to_string(),
            self.degraded.to_string(),
            self.stopped.to_string(),
            self.endpoints_up.to_string(),
            self.endpoints_total.to_string(),
            self.alerts_warning.to_string(),
            self.alerts_critical.to_string(),
            self.messages_stored.to_string(),
            self.deliveries_unresolved.to_string(),
            self.deliveries_queued.to_string(),
            self.acked.to_string(),
            self.received.to_string(),
            self.bytes_sent.to_string(),
            self.bytes_received.to_string(),
            self.e2e_count.to_string(),
            self.e2e_p50_ms.to_string(),
            self.e2e_p99_ms.to_string(),
            self.episodes_active.to_string(),
        ]
        .join(",")
    }
}

/// One row of `channels.csv`: a channel's numbers from the engine and from its peers.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChanSample {
    pub(crate) t_s: u64,
    pub(crate) incarnation: u32,
    pub(crate) channel: String,
    pub(crate) kind: &'static str,
    /// When the channel instance started; a change means the channel restarted.
    pub(crate) instance: String,
    pub(crate) state: String,
    pub(crate) session: String,
    pub(crate) retry_count: u64,
    // Engine, bridges.
    pub(crate) bytes_a_to_b: u64,
    pub(crate) bytes_b_to_a: u64,
    pub(crate) messages_a_to_b: u64,
    pub(crate) messages_b_to_a: u64,
    pub(crate) buffered_bytes: u64,
    pub(crate) overflow: u64,
    // Engine, message channels.
    pub(crate) received: u64,
    pub(crate) duplicates: u64,
    pub(crate) invalid: u64,
    pub(crate) aborted: u64,
    pub(crate) queued: u64,
    pub(crate) acknowledged: u64,
    pub(crate) unresolved: u64,
    // Peers.
    pub(crate) peer_acked: u64,
    pub(crate) peer_received: u64,
    pub(crate) peer_bytes_sent: u64,
    pub(crate) peer_bytes_received: u64,
    pub(crate) peer_records: u64,
    /// Median and 99th percentile delay from the first send to receipt, milliseconds.
    pub(crate) lat_p50_ms: u64,
    pub(crate) lat_p99_ms: u64,
}

impl ChanSample {
    pub(crate) const HEADER: &'static str = "t_s,incarnation,channel,kind,state,session,retry_count,\
bytes_a_to_b,bytes_b_to_a,messages_a_to_b,messages_b_to_a,buffered_bytes,overflow,received,duplicates,\
invalid,aborted,queued,acknowledged,unresolved,peer_acked,peer_received,peer_bytes_sent,\
peer_bytes_received,peer_records,lat_p50_ms,lat_p99_ms";

    pub(crate) fn csv(&self) -> String {
        [
            self.t_s.to_string(),
            self.incarnation.to_string(),
            self.channel.clone(),
            self.kind.to_owned(),
            self.state.clone(),
            self.session.clone(),
            self.retry_count.to_string(),
            self.bytes_a_to_b.to_string(),
            self.bytes_b_to_a.to_string(),
            self.messages_a_to_b.to_string(),
            self.messages_b_to_a.to_string(),
            self.buffered_bytes.to_string(),
            self.overflow.to_string(),
            self.received.to_string(),
            self.duplicates.to_string(),
            self.invalid.to_string(),
            self.aborted.to_string(),
            self.queued.to_string(),
            self.acknowledged.to_string(),
            self.unresolved.to_string(),
            self.peer_acked.to_string(),
            self.peer_received.to_string(),
            self.peer_bytes_sent.to_string(),
            self.peer_bytes_received.to_string(),
            self.peer_records.to_string(),
            self.lat_p50_ms.to_string(),
            self.lat_p99_ms.to_string(),
        ]
        .join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_have_as_many_cells_as_the_header() {
        let cells = |s: &str| s.split(',').count();
        assert_eq!(cells(&Sample::default().csv()), cells(Sample::HEADER));
        assert_eq!(
            cells(&ChanSample::default().csv()),
            cells(ChanSample::HEADER)
        );
    }

    #[test]
    fn sizes_are_written_in_mebibytes() {
        let sample = Sample {
            footprint_kib: 30_720,
            rss_kib: 61_440 + 512,
            cpu_tenths: 123,
            ..Sample::default()
        };
        let row = sample.csv();
        let cells: Vec<&str> = row.split(',').collect();
        assert_eq!(cells[5], "30.0");
        assert_eq!(cells[6], "60.5");
        assert_eq!(cells[7], "12.3");
        assert_eq!(sample.memory_kib(), 30_720);
        let without = Sample {
            rss_kib: 5_000,
            ..Sample::default()
        };
        assert_eq!(without.memory_kib(), 5_000, "the resident size stands in");
    }
}
