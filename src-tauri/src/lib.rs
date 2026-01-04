use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

#[cfg(windows)]
use windows::{
    core::PCSTR,
    Win32::Foundation::{CloseHandle, HANDLE},
    Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, OpenFileMappingA,
        FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE as PAGE_RW,
    },
    Win32::System::Threading::{
        OpenProcess, CreateRemoteThread, PROCESS_ALL_ACCESS, GetExitCodeThread,
    },
    Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
    Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32First, Process32Next,
        PROCESSENTRY32, TH32CS_SNAPPROCESS,
    },
    Win32::System::Diagnostics::Debug::WriteProcessMemory,
};

// Constants matching DLL
pub const MAX_BAN_TARGETS: usize = 10;
pub const MAX_BATTLETAG_LEN: usize = 64;
pub const MAX_MEMO_LEN: usize = 128;
pub const MAX_ROOM_USERS: usize = 8;
pub const MAX_NICKNAME_LEN: usize = 32;
pub const MAX_ROOM_NAME_LEN: usize = 64;
pub const MAX_MAP_NAME_LEN: usize = 128;
pub const MAX_HOST_NAME_LEN: usize = 64;
pub const SHARED_MEMORY_NAME: &str = "Local\\SCMonitorAutoBan";
pub const ROOMUSERS_MEMORY_NAME: &str = "Local\\SCMonitorRoomUsers";
pub const ROOMINFO_MEMORY_NAME: &str = "Local\\SCMonitorRoomInfo";
pub const LATENCY_MEMORY_NAME: &str = "Local\\SCMonitorLatency";
pub const MAX_PEERS: usize = 8;
pub const SHARED_MEMORY_VERSION: u32 = 2;

// Shared memory structure (must match DLL exactly)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AutoBanConfig {
    pub version: u32,
    pub enabled: u32,
    pub target_count: u32,
    pub reserved: u32,
    pub targets: [[u8; MAX_BATTLETAG_LEN]; MAX_BAN_TARGETS],
    pub memos: [[u8; MAX_MEMO_LEN]; MAX_BAN_TARGETS],
    pub total_kicks: u32,
    pub successful_kicks: u32,
    pub last_kick_time: u32,
    pub last_kicked_battletag: [u8; MAX_BATTLETAG_LEN],
}

impl Default for AutoBanConfig {
    fn default() -> Self {
        Self {
            version: SHARED_MEMORY_VERSION,
            enabled: 1, // Always enabled
            target_count: 0,
            reserved: 0,
            targets: [[0u8; MAX_BATTLETAG_LEN]; MAX_BAN_TARGETS],
            memos: [[0u8; MAX_MEMO_LEN]; MAX_BAN_TARGETS],
            total_kicks: 0,
            successful_kicks: 0,
            last_kick_time: 0,
            last_kicked_battletag: [0u8; MAX_BATTLETAG_LEN],
        }
    }
}

// Room users shared memory structures (must match DLL exactly)
pub const MAX_IP_LEN: usize = 16;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RoomUserData {
    pub active: u32,
    pub player_id: u32,
    pub nickname: [u8; MAX_NICKNAME_LEN],
    pub battletag: [u8; MAX_BATTLETAG_LEN],
    pub ip_address: [u8; MAX_IP_LEN],
    pub port: u16,
    pub flags: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RoomUsersData {
    pub version: u32,
    pub user_count: u32,
    pub last_update: u32,
    pub reserved: u32,
    pub users: [RoomUserData; MAX_ROOM_USERS],
}

// Room info shared memory structure (must match DLL exactly)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RoomInfoData {
    pub version: u32,
    pub in_room: u32,
    pub last_update: u32,
    pub reserved: u32,
    pub room_name: [u8; MAX_ROOM_NAME_LEN],
    pub map_name: [u8; MAX_MAP_NAME_LEN],
    pub host_name: [u8; MAX_HOST_NAME_LEN],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RoomInfo {
    #[serde(rename = "inRoom")]
    pub in_room: bool,
    #[serde(rename = "roomName")]
    pub room_name: String,
    #[serde(rename = "mapName")]
    pub map_name: String,
    #[serde(rename = "hostName")]
    pub host_name: String,
}

// Latency shared memory structures (must match DLL exactly)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PeerLatencyData {
    pub active: u32,
    pub ip_addr: u32,
    pub port: u16,
    pub reserved: u16,
    pub last_send_time: u64,
    pub last_recv_time: u64,
    pub current_rtt_us: u32,
    pub avg_rtt_us: u32,
    pub sample_count: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LatencyDataRaw {
    pub version: u32,
    pub peer_count: u32,
    pub last_update: u32,
    pub reserved: u32,
    pub peers: [PeerLatencyData; MAX_PEERS],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PeerLatency {
    #[serde(rename = "ipAddress")]
    pub ip_address: String,
    pub port: u16,
    #[serde(rename = "pingMs")]
    pub ping_ms: u32,
    #[serde(rename = "avgPingMs")]
    pub avg_ping_ms: u32,
    #[serde(rename = "sampleCount")]
    pub sample_count: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RoomUser {
    pub nickname: String,
    pub battletag: String,
    #[serde(rename = "playerId")]
    pub player_id: u32,
    #[serde(rename = "ipAddress")]
    pub ip_address: String,
    pub port: u16,
}

// App state
pub struct AppState {
    #[cfg(windows)]
    shared_memory_handle: Mutex<Option<HANDLE>>,
    #[cfg(windows)]
    shared_memory_ptr: Mutex<Option<*mut AutoBanConfig>>,
    // Local blacklist for when shared memory isn't available
    local_blacklist: Mutex<Vec<String>>,
    // Mock room users (will be populated from DLL in future)
    room_users: Mutex<Vec<RoomUser>>,
}

unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

impl Default for AppState {
    fn default() -> Self {
        Self {
            #[cfg(windows)]
            shared_memory_handle: Mutex::new(None),
            #[cfg(windows)]
            shared_memory_ptr: Mutex::new(None),
            local_blacklist: Mutex::new(Vec::new()),
            room_users: Mutex::new(Vec::new()),
        }
    }
}

#[cfg(windows)]
impl AppState {
    fn ensure_shared_memory(&self) -> bool {
        let mut handle = self.shared_memory_handle.lock().unwrap();
        let mut ptr = self.shared_memory_ptr.lock().unwrap();

        if ptr.is_some() {
            return true;
        }

        unsafe {
            let name = format!("{}\0", SHARED_MEMORY_NAME);
            let name_pcstr = PCSTR::from_raw(name.as_ptr());

            // Try to open existing shared memory first
            let h = OpenFileMappingA(FILE_MAP_ALL_ACCESS.0, false, name_pcstr);

            let mapping_handle = if let Ok(h) = h {
                h
            } else {
                // Create new shared memory
                let size = std::mem::size_of::<AutoBanConfig>() as u32;
                match CreateFileMappingA(
                    HANDLE::default(),
                    None,
                    PAGE_READWRITE,
                    0,
                    size,
                    name_pcstr,
                ) {
                    Ok(h) => h,
                    Err(_) => return false,
                }
            };

            let map_ptr = MapViewOfFile(mapping_handle, FILE_MAP_ALL_ACCESS, 0, 0, 0);

            if map_ptr.Value.is_null() {
                let _ = CloseHandle(mapping_handle);
                return false;
            }

            let config_ptr = map_ptr.Value as *mut AutoBanConfig;

            // Initialize if new (version == 0)
            if (*config_ptr).version == 0 {
                *config_ptr = AutoBanConfig::default();
            }

            *handle = Some(mapping_handle);
            *ptr = Some(config_ptr);
            true
        }
    }

    fn get_config(&self) -> Option<AutoBanConfig> {
        if !self.ensure_shared_memory() {
            return None;
        }

        let ptr = self.shared_memory_ptr.lock().unwrap();
        ptr.map(|p| unsafe { *p })
    }

    fn update_config<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut AutoBanConfig),
    {
        if !self.ensure_shared_memory() {
            return false;
        }

        let ptr = self.shared_memory_ptr.lock().unwrap();
        if let Some(p) = *ptr {
            unsafe {
                f(&mut *p);
            }
            true
        } else {
            false
        }
    }
}

fn battletag_to_bytes(battletag: &str) -> [u8; MAX_BATTLETAG_LEN] {
    let mut bytes = [0u8; MAX_BATTLETAG_LEN];
    let src = battletag.as_bytes();
    let len = src.len().min(MAX_BATTLETAG_LEN - 1);
    bytes[..len].copy_from_slice(&src[..len]);
    bytes
}

fn bytes_to_battletag(bytes: &[u8; MAX_BATTLETAG_LEN]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(MAX_BATTLETAG_LEN);
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn memo_to_bytes(memo: &str) -> [u8; MAX_MEMO_LEN] {
    let mut bytes = [0u8; MAX_MEMO_LEN];
    let src = memo.as_bytes();
    let len = src.len().min(MAX_MEMO_LEN - 1);
    bytes[..len].copy_from_slice(&src[..len]);
    bytes
}

fn bytes_to_memo(bytes: &[u8; MAX_MEMO_LEN]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(MAX_MEMO_LEN);
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlacklistEntry {
    pub battletag: String,
    pub memo: String,
}

// Blacklist file path (in app data directory)
fn get_blacklist_file_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("r-launcher");
    fs::create_dir_all(&path).ok();
    path.push("blacklist.json");
    path
}

// Load blacklist from file
fn load_blacklist_from_file() -> Vec<BlacklistEntry> {
    let path = get_blacklist_file_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

// Save blacklist to file
fn save_blacklist_to_file(entries: &[BlacklistEntry]) -> Result<(), String> {
    let path = get_blacklist_file_path();
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
    file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

// Sync blacklist to shared memory
#[cfg(windows)]
fn sync_blacklist_to_shared_memory(state: &AppState, entries: &[BlacklistEntry]) {
    state.update_config(|config| {
        // Clear existing
        config.target_count = 0;
        for i in 0..MAX_BAN_TARGETS {
            config.targets[i] = [0u8; MAX_BATTLETAG_LEN];
            config.memos[i] = [0u8; MAX_MEMO_LEN];
        }

        // Add entries
        for (i, entry) in entries.iter().take(MAX_BAN_TARGETS).enumerate() {
            config.targets[i] = battletag_to_bytes(&entry.battletag);
            config.memos[i] = memo_to_bytes(&entry.memo);
            config.target_count = (i + 1) as u32;
        }
    });
}

#[tauri::command]
fn is_connected(state: State<AppState>) -> bool {
    #[cfg(windows)]
    {
        state.ensure_shared_memory()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[tauri::command]
fn get_blacklist(_state: State<AppState>) -> Vec<BlacklistEntry> {
    load_blacklist_from_file()
}

#[tauri::command]
fn add_to_blacklist(state: State<AppState>, battletag: String, memo: Option<String>) -> Result<(), String> {
    if battletag.trim().is_empty() {
        return Err("Battletag cannot be empty".to_string());
    }

    let mut entries = load_blacklist_from_file();

    // Check for duplicates
    if entries.iter().any(|e| e.battletag == battletag) {
        return Err("Battletag already in blacklist".to_string());
    }

    // Check max limit
    if entries.len() >= MAX_BAN_TARGETS {
        return Err("Blacklist is full".to_string());
    }

    // Add new entry
    entries.push(BlacklistEntry {
        battletag,
        memo: memo.unwrap_or_default(),
    });

    // Save to file
    save_blacklist_to_file(&entries)?;

    // Sync to shared memory
    #[cfg(windows)]
    sync_blacklist_to_shared_memory(&state, &entries);

    Ok(())
}

#[tauri::command]
fn remove_from_blacklist(state: State<AppState>, battletag: String) -> Result<(), String> {
    let mut entries = load_blacklist_from_file();

    let original_len = entries.len();
    entries.retain(|e| e.battletag != battletag);

    if entries.len() == original_len {
        return Err("Battletag not found in blacklist".to_string());
    }

    // Save to file
    save_blacklist_to_file(&entries)?;

    // Sync to shared memory
    #[cfg(windows)]
    sync_blacklist_to_shared_memory(&state, &entries);

    Ok(())
}

#[tauri::command]
fn update_blacklist_memo(state: State<AppState>, battletag: String, memo: String) -> Result<(), String> {
    let mut entries = load_blacklist_from_file();

    let entry = entries.iter_mut().find(|e| e.battletag == battletag);
    match entry {
        Some(e) => {
            e.memo = memo;
        }
        None => return Err("Battletag not found in blacklist".to_string()),
    }

    // Save to file
    save_blacklist_to_file(&entries)?;

    // Sync to shared memory
    #[cfg(windows)]
    sync_blacklist_to_shared_memory(&state, &entries);

    Ok(())
}

#[tauri::command]
fn get_room_users(_state: State<AppState>) -> Vec<RoomUser> {
    #[cfg(windows)]
    {
        unsafe {
            let name = format!("{}\0", ROOMUSERS_MEMORY_NAME);
            let name_pcstr = PCSTR::from_raw(name.as_ptr());

            // Try to open existing shared memory
            let h = match OpenFileMappingA(FILE_MAP_ALL_ACCESS.0, false, name_pcstr) {
                Ok(h) => h,
                Err(_) => return Vec::new(), // Shared memory not created yet
            };

            let map_ptr = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if map_ptr.Value.is_null() {
                let _ = CloseHandle(h);
                return Vec::new();
            }

            let data_ptr = map_ptr.Value as *const RoomUsersData;
            let data = &*data_ptr;

            let mut users = Vec::new();
            for i in 0..MAX_ROOM_USERS {
                if data.users[i].active == 1 {
                    // Extract nickname
                    let nick_end = data.users[i].nickname
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(MAX_NICKNAME_LEN);
                    let nickname = String::from_utf8_lossy(&data.users[i].nickname[..nick_end]).to_string();

                    // Extract battletag
                    let tag_end = data.users[i].battletag
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(MAX_BATTLETAG_LEN);
                    let battletag = String::from_utf8_lossy(&data.users[i].battletag[..tag_end]).to_string();

                    // Extract IP address
                    let ip_end = data.users[i].ip_address
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(MAX_IP_LEN);
                    let ip_address = String::from_utf8_lossy(&data.users[i].ip_address[..ip_end]).to_string();

                    users.push(RoomUser {
                        nickname,
                        battletag,
                        player_id: data.users[i].player_id,
                        ip_address,
                        port: data.users[i].port,
                    });
                }
            }

            let _ = CloseHandle(h);
            users
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[tauri::command]
fn get_room_info() -> RoomInfo {
    #[cfg(windows)]
    {
        unsafe {
            let name = format!("{}\0", ROOMINFO_MEMORY_NAME);
            let name_pcstr = PCSTR::from_raw(name.as_ptr());

            // Try to open existing shared memory
            let h = match OpenFileMappingA(FILE_MAP_ALL_ACCESS.0, false, name_pcstr) {
                Ok(h) => h,
                Err(_) => {
                    return RoomInfo {
                        in_room: false,
                        room_name: String::new(),
                        map_name: String::new(),
                        host_name: String::new(),
                    };
                }
            };

            let map_ptr = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if map_ptr.Value.is_null() {
                let _ = CloseHandle(h);
                return RoomInfo {
                    in_room: false,
                    room_name: String::new(),
                    map_name: String::new(),
                    host_name: String::new(),
                };
            }

            let data_ptr = map_ptr.Value as *const RoomInfoData;
            let data = &*data_ptr;

            // Extract room name
            let room_name_end = data.room_name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(MAX_ROOM_NAME_LEN);
            let room_name = String::from_utf8_lossy(&data.room_name[..room_name_end]).to_string();

            // Extract map name
            let map_name_end = data.map_name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(MAX_MAP_NAME_LEN);
            let map_name = String::from_utf8_lossy(&data.map_name[..map_name_end]).to_string();

            // Extract host name
            let host_name_end = data.host_name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(MAX_HOST_NAME_LEN);
            let host_name = String::from_utf8_lossy(&data.host_name[..host_name_end]).to_string();

            let _ = CloseHandle(h);

            RoomInfo {
                in_room: data.in_room == 1,
                room_name,
                map_name,
                host_name,
            }
        }
    }
    #[cfg(not(windows))]
    {
        RoomInfo {
            in_room: false,
            room_name: String::new(),
            map_name: String::new(),
            host_name: String::new(),
        }
    }
}

#[tauri::command]
fn get_peer_latencies() -> Vec<PeerLatency> {
    #[cfg(windows)]
    {
        unsafe {
            let name = format!("{}\0", LATENCY_MEMORY_NAME);
            let name_pcstr = PCSTR::from_raw(name.as_ptr());

            // Try to open existing shared memory
            let h = match OpenFileMappingA(FILE_MAP_ALL_ACCESS.0, false, name_pcstr) {
                Ok(h) => h,
                Err(_) => return Vec::new(), // Shared memory not created yet
            };

            let map_ptr = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if map_ptr.Value.is_null() {
                let _ = CloseHandle(h);
                return Vec::new();
            }

            let data_ptr = map_ptr.Value as *const LatencyDataRaw;
            let data = &*data_ptr;

            let mut latencies = Vec::new();
            for i in 0..MAX_PEERS {
                if data.peers[i].active == 1 && data.peers[i].sample_count > 0 {
                    // Convert IP address from u32 to string
                    let ip = data.peers[i].ip_addr;
                    let ip_address = format!(
                        "{}.{}.{}.{}",
                        ip & 0xFF,
                        (ip >> 8) & 0xFF,
                        (ip >> 16) & 0xFF,
                        (ip >> 24) & 0xFF
                    );

                    // Convert microseconds to milliseconds
                    let ping_ms = data.peers[i].current_rtt_us / 1000;
                    let avg_ping_ms = data.peers[i].avg_rtt_us / 1000;

                    latencies.push(PeerLatency {
                        ip_address,
                        port: data.peers[i].port,
                        ping_ms,
                        avg_ping_ms,
                        sample_count: data.peers[i].sample_count,
                    });
                }
            }

            let _ = CloseHandle(h);
            latencies
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[tauri::command]
async fn get_dll_logs(last_lines: Option<usize>) -> Vec<String> {
    let lines_to_read = last_lines.unwrap_or(100);

    // Run file I/O in blocking thread pool to avoid blocking async runtime
    tauri::async_runtime::spawn_blocking(move || {
        // Get TEMP directory
        let temp_dir = std::env::var("TEMP").unwrap_or_else(|_| {
            std::env::var("TMP").unwrap_or_else(|_| "C:\\Windows\\Temp".to_string())
        });

        let log_path = format!("{}\\sc_monitor_dll.log", temp_dir);

        match fs::File::open(&log_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                let all_lines: Vec<String> = reader
                    .lines()
                    .filter_map(|l| l.ok())
                    .collect();

                // Return last N lines
                let start = all_lines.len().saturating_sub(lines_to_read);
                all_lines[start..].to_vec()
            }
            Err(_) => Vec::new(),
        }
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn is_dll_injected() -> bool {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(windows)]
        {
            // Find StarCraft process
            let pid = match find_process_by_name("StarCraft.exe") {
                Some(pid) => pid,
                None => return false,
            };

            // Check if sc_hook_dll.dll is loaded in the process
            return is_module_loaded_in_process(pid, "sc_hook_dll.dll");
        }

        #[cfg(not(windows))]
        false
    })
    .await
    .unwrap_or(false)
}

#[cfg(windows)]
fn is_module_loaded_in_process(pid: u32, module_name: &str) -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32First, Module32Next,
        MODULEENTRY32, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
    };

    unsafe {
        // Create snapshot of modules
        let snapshot = match CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
            pid,
        ) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let mut entry = MODULEENTRY32 {
            dwSize: std::mem::size_of::<MODULEENTRY32>() as u32,
            ..Default::default()
        };

        if Module32First(snapshot, &mut entry).is_ok() {
            loop {
                let name = std::ffi::CStr::from_ptr(entry.szModule.as_ptr())
                    .to_string_lossy()
                    .to_lowercase();

                if name.contains(&module_name.to_lowercase()) {
                    let _ = CloseHandle(snapshot);
                    return true;
                }

                if Module32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        false
    }
}

#[tauri::command]
fn clear_dll_logs() -> Result<(), String> {
    let temp_dir = std::env::var("TEMP").unwrap_or_else(|_| {
        std::env::var("TMP").unwrap_or_else(|_| "C:\\Windows\\Temp".to_string())
    });

    let log_path = format!("{}\\sc_monitor_dll.log", temp_dir);

    fs::write(&log_path, "").map_err(|e| e.to_string())
}

#[cfg(windows)]
fn find_process_by_name(name: &str) -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;

        let mut entry = PROCESSENTRY32 {
            dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
            ..Default::default()
        };

        if Process32First(snapshot, &mut entry).is_ok() {
            loop {
                let exe_name = std::ffi::CStr::from_ptr(entry.szExeFile.as_ptr())
                    .to_string_lossy();

                if exe_name.to_lowercase().contains(&name.to_lowercase()) {
                    let _ = CloseHandle(snapshot);
                    return Some(entry.th32ProcessID);
                }

                if Process32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        None
    }
}

#[cfg(windows)]
fn inject_dll_into_process(pid: u32, dll_path: &str) -> Result<(), String> {
    println!("[INJECT] Starting injection into PID: {}", pid);
    println!("[INJECT] DLL path: {}", dll_path);

    unsafe {
        // Open the target process
        println!("[INJECT] Opening process with PROCESS_ALL_ACCESS...");
        let process = OpenProcess(PROCESS_ALL_ACCESS, false, pid)
            .map_err(|e| {
                println!("[INJECT] ERROR: Failed to open process: {}", e);
                format!("Failed to open process: {}", e)
            })?;
        println!("[INJECT] Process opened successfully: {:?}", process);

        // Get LoadLibraryA address
        println!("[INJECT] Getting kernel32.dll handle...");
        let kernel32 = GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr()))
            .map_err(|e| {
                println!("[INJECT] ERROR: Failed to get kernel32: {}", e);
                format!("Failed to get kernel32: {}", e)
            })?;
        println!("[INJECT] kernel32.dll handle: {:?}", kernel32);

        println!("[INJECT] Getting LoadLibraryA address...");
        let load_library = GetProcAddress(kernel32, PCSTR(b"LoadLibraryA\0".as_ptr()))
            .ok_or_else(|| {
                println!("[INJECT] ERROR: Failed to get LoadLibraryA address");
                "Failed to get LoadLibraryA address".to_string()
            })?;
        println!("[INJECT] LoadLibraryA address: {:?}", load_library);

        // Allocate memory in target process for DLL path
        let dll_path_bytes = dll_path.as_bytes();
        let path_len = dll_path_bytes.len() + 1;
        println!("[INJECT] DLL path length: {} bytes", path_len);

        println!("[INJECT] Allocating {} bytes in target process...", path_len);
        let remote_memory = VirtualAllocEx(
            process,
            None,
            path_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_RW,
        );

        if remote_memory.is_null() {
            println!("[INJECT] ERROR: VirtualAllocEx returned null");
            let _ = CloseHandle(process);
            return Err("Failed to allocate memory in target process".to_string());
        }
        println!("[INJECT] Remote memory allocated at: {:?}", remote_memory);

        // Write DLL path to target process
        println!("[INJECT] Writing DLL path to target process...");
        let mut bytes_written = 0;
        let write_result = WriteProcessMemory(
            process,
            remote_memory,
            dll_path_bytes.as_ptr() as *const _,
            path_len,
            Some(&mut bytes_written),
        );

        if write_result.is_err() {
            println!("[INJECT] ERROR: WriteProcessMemory failed");
            let _ = VirtualFreeEx(process, remote_memory, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("Failed to write DLL path to target process".to_string());
        }
        println!("[INJECT] Wrote {} bytes to target process", bytes_written);

        // Create remote thread to load the DLL
        println!("[INJECT] Creating remote thread...");
        let thread = CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute(load_library)),
            Some(remote_memory),
            0,
            None,
        );

        match thread {
            Ok(handle) => {
                println!("[INJECT] Remote thread created: {:?}", handle);
                println!("[INJECT] Waiting for thread to complete (5s timeout)...");
                let wait_result = windows::Win32::System::Threading::WaitForSingleObject(handle, 5000);
                println!("[INJECT] WaitForSingleObject result: {:?}", wait_result);

                // Check the return value of LoadLibraryA
                let mut exit_code: u32 = 0;
                if GetExitCodeThread(handle, &mut exit_code).is_ok() {
                    println!("[INJECT] LoadLibraryA returned: 0x{:08x}", exit_code);
                    if exit_code == 0 {
                        println!("[INJECT] ERROR: LoadLibraryA FAILED! DLL did not load.");
                        println!("[INJECT] Possible causes:");
                        println!("[INJECT]   - DLL has missing dependencies");
                        println!("[INJECT]   - DLL architecture mismatch");
                        println!("[INJECT]   - DLL path encoding issue");
                    } else {
                        println!("[INJECT] SUCCESS: DLL loaded at base address 0x{:08x}", exit_code);
                    }
                }

                let _ = CloseHandle(handle);
            }
            Err(e) => {
                println!("[INJECT] ERROR: CreateRemoteThread failed: {}", e);
                let _ = VirtualFreeEx(process, remote_memory, 0, MEM_RELEASE);
                let _ = CloseHandle(process);
                return Err(format!("Failed to create remote thread: {}", e));
            }
        }

        // Cleanup
        println!("[INJECT] Cleaning up...");
        let _ = VirtualFreeEx(process, remote_memory, 0, MEM_RELEASE);
        let _ = CloseHandle(process);

        println!("[INJECT] Injection completed successfully!");
        Ok(())
    }
}

#[tauri::command]
fn find_starcraft_process() -> Option<u32> {
    #[cfg(windows)]
    {
        find_process_by_name("StarCraft.exe")
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[tauri::command]
fn inject_dll(dll_path: String) -> Result<String, String> {
    println!("========================================");
    println!("[INJECT_DLL] Called with path: {}", dll_path);

    // Check if file exists
    let path = std::path::Path::new(&dll_path);
    if path.exists() {
        println!("[INJECT_DLL] File EXISTS at path");
        if let Ok(metadata) = std::fs::metadata(&dll_path) {
            println!("[INJECT_DLL] File size: {} bytes", metadata.len());
        }
    } else {
        println!("[INJECT_DLL] ERROR: File does NOT exist at path!");
        return Err(format!("DLL file not found: {}", dll_path));
    }

    #[cfg(windows)]
    {
        // Find StarCraft process
        println!("[INJECT_DLL] Searching for StarCraft.exe...");
        let pid = find_process_by_name("StarCraft.exe")
            .ok_or_else(|| {
                println!("[INJECT_DLL] ERROR: StarCraft.exe not found");
                "StarCraft.exe not found. Please start the game first.".to_string()
            })?;
        println!("[INJECT_DLL] Found StarCraft.exe with PID: {}", pid);

        // Inject DLL
        println!("[INJECT_DLL] Starting injection...");
        inject_dll_into_process(pid, &dll_path)?;

        println!("[INJECT_DLL] SUCCESS!");
        println!("========================================");
        Ok(format!("DLL injected successfully into StarCraft.exe (PID: {})", pid))
    }
    #[cfg(not(windows))]
    {
        Err("DLL injection is only supported on Windows".to_string())
    }
}

#[tauri::command]
fn get_default_dll_path() -> String {
    println!("[GET_DLL_PATH] Searching for DLL...");

    // Try to find the DLL in common locations
    // Note: Use ASCII-only paths to avoid LoadLibraryA encoding issues
    let possible_paths = [
        // ASCII-only path (preferred)
        r"C:\temp\sc_hook_dll.dll",
        // Relative to the app
        "sc_hook_dll.dll",
    ];

    for path in possible_paths {
        println!("[GET_DLL_PATH] Checking: {}", path);
        if std::path::Path::new(path).exists() {
            println!("[GET_DLL_PATH] FOUND: {}", path);
            return path.to_string();
        } else {
            println!("[GET_DLL_PATH] Not found at this path");
        }
    }

    println!("[GET_DLL_PATH] No DLL found, returning default: {}", possible_paths[0]);
    possible_paths[0].to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            is_connected,
            get_blacklist,
            add_to_blacklist,
            remove_from_blacklist,
            update_blacklist_memo,
            get_room_users,
            get_room_info,
            get_peer_latencies,
            get_dll_logs,
            is_dll_injected,
            clear_dll_logs,
            find_starcraft_process,
            inject_dll,
            get_default_dll_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
