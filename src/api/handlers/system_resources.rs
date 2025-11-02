use crate::api::types::SystemResourcesResponse;
use axum::{http::StatusCode, Json};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, RefreshKind, System};

/// GET /api/system/resources - Get system and process resource usage
pub async fn get_system_resources() -> (StatusCode, Json<SystemResourcesResponse>) {
    // Create system instance with refresh settings
    let mut sys = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything())
            .with_processes(ProcessRefreshKind::everything()),
    );

    // Wait a moment and refresh CPU to get accurate readings
    std::thread::sleep(std::time::Duration::from_millis(200));
    sys.refresh_cpu_all();
    sys.refresh_memory();

    // Get system-wide metrics
    let system_cpu_percent = sys.global_cpu_usage();
    let system_ram_total_bytes = sys.total_memory();
    let system_ram_used_bytes = sys.used_memory();
    let system_ram_percent = if system_ram_total_bytes > 0 {
        (system_ram_used_bytes as f32 / system_ram_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    // Get current process metrics
    let pid = sysinfo::get_current_pid().ok();
    let (process_cpu_percent, process_ram_bytes) = if let Some(pid) = pid {
        if let Some(process) = sys.process(pid) {
            (process.cpu_usage(), process.memory())
        } else {
            (0.0, 0)
        }
    } else {
        (0.0, 0)
    };

    // Get load average (Unix-like systems only)
    let load_average_1m = System::load_average().one;

    let response = SystemResourcesResponse {
        system_cpu_percent,
        system_ram_percent,
        system_ram_total_bytes,
        system_ram_used_bytes,
        process_cpu_percent,
        process_ram_bytes,
        load_average_1m: if load_average_1m > 0.0 {
            Some(load_average_1m)
        } else {
            None
        },
    };

    (StatusCode::OK, Json(response))
}
