use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

// Logging utility for debugging
fn log_to_file(message: &str) {
    // Ensure C:\temp directory exists
    let _ = std::fs::create_dir_all("C:\\temp");

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\temp\\r-launcher-debug.log")
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let _ = writeln!(file, "[{}] {}", timestamp, message);
    }
}

#[cfg(windows)]
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::Foundation::{CloseHandle, HANDLE, GetLastError},
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
pub const MAX_ROOM_USERS: usize = 100;
pub const MAX_NICKNAME_LEN: usize = 32;
pub const MAX_ROOM_NAME_LEN: usize = 64;
pub const MAX_MAP_NAME_LEN: usize = 128;
pub const MAX_HOST_NAME_LEN: usize = 64;
pub const MAX_FORCE_NAME_LEN: usize = 64;
pub const MAX_FORCES: usize = 4;
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
    pub force_count: u32,
    pub room_id: [u8; 16],
    pub room_name: [u8; MAX_ROOM_NAME_LEN],
    pub map_name: [u8; MAX_MAP_NAME_LEN],
    pub host_name: [u8; MAX_HOST_NAME_LEN],
    pub force_names: [[u8; MAX_FORCE_NAME_LEN]; MAX_FORCES],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RoomInfo {
    #[serde(rename = "inRoom")]
    pub in_room: bool,
    #[serde(rename = "roomId")]
    pub room_id: String,
    #[serde(rename = "roomName")]
    pub room_name: String,
    #[serde(rename = "mapName")]
    pub map_name: String,
    #[serde(rename = "hostName")]
    pub host_name: String,
    #[serde(rename = "forceNames")]
    pub force_names: Vec<String>,
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

// Room history entry structure
#[derive(Serialize, Deserialize, Clone)]
pub struct RoomHistoryEntry {
    pub timestamp: u64,
    #[serde(rename = "roomInfo")]
    pub room_info: RoomInfo,
    pub users: Vec<RoomUser>,
}

// Maximum history entries to keep
pub const MAX_HISTORY_ENTRIES: usize = 50;

// History file path (in app data directory)
fn get_history_file_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("r-launcher");
    fs::create_dir_all(&path).ok();
    path.push("room_history.json");
    path
}

// Load history from file
fn load_history_from_file() -> Vec<RoomHistoryEntry> {
    let path = get_history_file_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

// Save history to file
fn save_history_to_file(entries: &[RoomHistoryEntry]) -> Result<(), String> {
    let path = get_history_file_path();
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
    file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
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

// Write blacklist targets to a simple text file for DLL to read (no entry limit)
fn write_blacklist_targets_file(entries: &[BlacklistEntry]) {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("r-launcher");
    let _ = fs::create_dir_all(&path);
    path.push("blacklist_targets.txt");
    let content: String = entries.iter().map(|e| e.battletag.as_str()).collect::<Vec<_>>().join("\n");
    let _ = fs::write(&path, content);
}

// Sync blacklist to shared memory (stats + up to MAX_BAN_TARGETS for legacy)
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

    // Add new entry
    entries.push(BlacklistEntry {
        battletag,
        memo: memo.unwrap_or_default(),
    });

    // Save to file
    save_blacklist_to_file(&entries)?;

    // Write targets file for DLL (unlimited)
    write_blacklist_targets_file(&entries);

    // Sync to shared memory (legacy, up to MAX_BAN_TARGETS)
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

    // Write targets file for DLL (unlimited)
    write_blacklist_targets_file(&entries);

    // Sync to shared memory (legacy, up to MAX_BAN_TARGETS)
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

    // Write targets file for DLL (unlimited)
    write_blacklist_targets_file(&entries);

    // Sync to shared memory (legacy, up to MAX_BAN_TARGETS)
    #[cfg(windows)]
    sync_blacklist_to_shared_memory(&state, &entries);

    Ok(())
}

// ============================================================================
// Room History Commands
// ============================================================================

#[tauri::command]
fn get_room_history() -> Vec<RoomHistoryEntry> {
    load_history_from_file()
}

#[tauri::command]
fn add_room_history(entry: RoomHistoryEntry) -> Result<(), String> {
    let mut entries = load_history_from_file();

    // Check if entry with same timestamp exists (update case)
    if let Some(existing) = entries.iter_mut().find(|e| e.timestamp == entry.timestamp) {
        // Update existing entry
        *existing = entry;
    } else {
        // Add new entry at the beginning (newest first)
        entries.insert(0, entry);

        // Limit to MAX_HISTORY_ENTRIES
        if entries.len() > MAX_HISTORY_ENTRIES {
            entries.truncate(MAX_HISTORY_ENTRIES);
        }
    }

    save_history_to_file(&entries)
}

#[tauri::command]
fn clear_room_history() -> Result<(), String> {
    save_history_to_file(&[])
}

#[tauri::command]
fn remove_room_history(timestamp: u64) -> Result<(), String> {
    let mut entries = load_history_from_file();
    entries.retain(|e| e.timestamp != timestamp);
    save_history_to_file(&entries)
}

// ============================================================================
// Room Users Commands
// ============================================================================

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
                        room_id: String::new(),
                        room_name: String::new(),
                        map_name: String::new(),
                        host_name: String::new(),
                        force_names: Vec::new(),
                    };
                }
            };

            let map_ptr = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            if map_ptr.Value.is_null() {
                let _ = CloseHandle(h);
                return RoomInfo {
                    in_room: false,
                    room_id: String::new(),
                    room_name: String::new(),
                    map_name: String::new(),
                    host_name: String::new(),
                    force_names: Vec::new(),
                };
            }

            let data_ptr = map_ptr.Value as *const RoomInfoData;
            let data = &*data_ptr;

            // Extract room ID
            let id_end = data.room_id.iter().position(|&b| b == 0).unwrap_or(16);
            let room_id = if id_end > 0 {
                data.room_id[..id_end].iter().map(|b| format!("{:02x}", b)).collect()
            } else {
                String::new()
            };

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

            // Extract force names
            let mut force_names = Vec::new();
            for i in 0..data.force_count.min(MAX_FORCES as u32) as usize {
                let force_name_end = data.force_names[i]
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(MAX_FORCE_NAME_LEN);
                let force_name = String::from_utf8_lossy(&data.force_names[i][..force_name_end]).to_string();
                if !force_name.is_empty() {
                    force_names.push(force_name);
                }
            }

            let _ = CloseHandle(h);

            RoomInfo {
                in_room: data.in_room == 1,
                room_id,
                room_name,
                map_name,
                host_name,
                force_names,
            }
        }
    }
    #[cfg(not(windows))]
    {
        RoomInfo {
            in_room: false,
            room_id: String::new(),
            room_name: String::new(),
            map_name: String::new(),
            host_name: String::new(),
            force_names: Vec::new(),
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
async fn is_dll_injected() -> bool {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(windows)]
        {
            // Find StarCraft process
            let pid = match find_process_by_name("StarCraft.exe") {
                Some(pid) => pid,
                None => {
                    log_to_file("[IS_DLL_INJECTED] StarCraft.exe not found");
                    return false;
                }
            };

            // Check if sc_hook_dll.dll is loaded in the process
            let result = is_module_loaded_in_process(pid, "sc_hook_dll.dll");
            log_to_file(&format!("[IS_DLL_INJECTED] PID: {}, Result: {}", pid, result));
            return result;
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

    log_to_file(&format!("[IS_MODULE_LOADED] Checking for module '{}' in PID {}", module_name, pid));

    unsafe {
        // Create snapshot of modules
        let snapshot = match CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
            pid,
        ) {
            Ok(h) => h,
            Err(e) => {
                log_to_file(&format!("[IS_MODULE_LOADED] Failed to create snapshot: {:?}", e));
                return false;
            }
        };

        let mut entry = MODULEENTRY32 {
            dwSize: std::mem::size_of::<MODULEENTRY32>() as u32,
            ..Default::default()
        };

        let mut module_count = 0;

        if Module32First(snapshot, &mut entry).is_ok() {
            loop {
                let name = std::ffi::CStr::from_ptr(entry.szModule.as_ptr())
                    .to_string_lossy()
                    .to_lowercase();

                module_count += 1;

                // Log only DLL modules
                if name.ends_with(".dll") {
                    log_to_file(&format!("[IS_MODULE_LOADED] Found: {}", name));
                }

                // Check for multiple possible DLL name variations
                let target_lower = module_name.to_lowercase();
                if name.contains(&target_lower) ||
                   name.contains("sc_hook") ||
                   name.contains("sc-hook") {
                    log_to_file(&format!("[IS_MODULE_LOADED] ✓ MATCH! '{}' found", name));
                    let _ = CloseHandle(snapshot);
                    return true;
                }

                if Module32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        log_to_file(&format!("[IS_MODULE_LOADED] ✗ '{}' NOT found (checked {} modules)", module_name, module_count));
        let _ = CloseHandle(snapshot);
        false
    }
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
    let msg = format!("[INJECT] Starting injection into PID: {}", pid);
    println!("{}", msg);
    log_to_file(&msg);

    let msg = format!("[INJECT] DLL path: {}", dll_path);
    println!("{}", msg);
    log_to_file(&msg);

    // Verify DLL exists before injection
    if !std::path::Path::new(dll_path).exists() {
        let msg = format!("[INJECT] ERROR: DLL file does not exist at path: {}", dll_path);
        println!("{}", msg);
        log_to_file(&msg);
        return Err(format!("DLL file not found: {}", dll_path));
    }
    log_to_file("[INJECT] DLL file verified to exist");

    // Try loading the DLL locally first to check for dependency issues
    unsafe {
        use windows::Win32::System::LibraryLoader::LoadLibraryW;
        use std::os::windows::ffi::OsStrExt;

        let wide_test_path: Vec<u16> = std::ffi::OsStr::new(dll_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        log_to_file("[INJECT] Testing DLL load locally first...");
        println!("[INJECT] Testing DLL load locally first...");

        match LoadLibraryW(PCWSTR(wide_test_path.as_ptr())) {
            Ok(handle) => {
                log_to_file("[INJECT] Local test load succeeded");
                println!("[INJECT] Local test load succeeded");
                // Free the library handle
                use windows::Win32::Foundation::FreeLibrary;
                let _ = FreeLibrary(handle);
            }
            Err(e) => {
                let error_code = GetLastError();
                let msg = format!("[INJECT] ERROR: Local test load failed! Error: {} (code: {:?})", e, error_code);
                println!("{}", msg);
                log_to_file(&msg);
                log_to_file("[INJECT] Common error codes:");
                log_to_file("[INJECT]   126 (0x7E) = Module not found (missing dependency)");
                log_to_file("[INJECT]   193 (0xC1) = Not a valid Win32 application (wrong architecture)");
                return Err(format!("DLL cannot be loaded locally: {} (Error code: {:?}). Check dependencies and architecture.", e, error_code));
            }
        }
    }

    unsafe {
        // Open the target process
        log_to_file("[INJECT] Opening process with PROCESS_ALL_ACCESS...");
        println!("[INJECT] Opening process with PROCESS_ALL_ACCESS...");
        let process = OpenProcess(PROCESS_ALL_ACCESS, false, pid)
            .map_err(|e| {
                let msg = format!("[INJECT] ERROR: Failed to open process: {}", e);
                println!("{}", msg);
                log_to_file(&msg);
                format!("Failed to open process: {}", e)
            })?;
        let msg = format!("[INJECT] Process opened successfully: {:?}", process);
        println!("{}", msg);
        log_to_file(&msg);

        // Get LoadLibraryW address (using W for Unicode support)
        log_to_file("[INJECT] Getting kernel32.dll handle...");
        println!("[INJECT] Getting kernel32.dll handle...");
        let kernel32 = GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr()))
            .map_err(|e| {
                let msg = format!("[INJECT] ERROR: Failed to get kernel32: {}", e);
                println!("{}", msg);
                log_to_file(&msg);
                format!("Failed to get kernel32: {}", e)
            })?;
        let msg = format!("[INJECT] kernel32.dll handle: {:?}", kernel32);
        println!("{}", msg);
        log_to_file(&msg);

        log_to_file("[INJECT] Getting LoadLibraryW address...");
        println!("[INJECT] Getting LoadLibraryW address...");
        let load_library = GetProcAddress(kernel32, PCSTR(b"LoadLibraryW\0".as_ptr()))
            .ok_or_else(|| {
                let msg = "[INJECT] ERROR: Failed to get LoadLibraryW address".to_string();
                println!("{}", msg);
                log_to_file(&msg);
                msg
            })?;
        let msg = format!("[INJECT] LoadLibraryW address: {:?}", load_library);
        println!("{}", msg);
        log_to_file(&msg);

        // Convert DLL path to wide string (UTF-16) for LoadLibraryW
        use std::os::windows::ffi::OsStrExt;
        let wide_path: Vec<u16> = std::ffi::OsStr::new(dll_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let path_len = wide_path.len() * 2; // 2 bytes per UTF-16 character
        let msg = format!("[INJECT] DLL path length: {} bytes (UTF-16)", path_len);
        println!("{}", msg);
        log_to_file(&msg);
        log_to_file(&format!("[INJECT] Allocating {} bytes in target process...", path_len));
        println!("[INJECT] Allocating {} bytes in target process...", path_len);
        let remote_memory = VirtualAllocEx(
            process,
            None,
            path_len,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_RW,
        );

        if remote_memory.is_null() {
            let msg = "[INJECT] ERROR: VirtualAllocEx returned null".to_string();
            println!("{}", msg);
            log_to_file(&msg);
            let _ = CloseHandle(process);
            return Err("Failed to allocate memory in target process".to_string());
        }
        let msg = format!("[INJECT] Remote memory allocated at: {:?}", remote_memory);
        println!("{}", msg);
        log_to_file(&msg);

        // Write DLL path to target process (as UTF-16)
        log_to_file("[INJECT] Writing DLL path (UTF-16) to target process...");
        println!("[INJECT] Writing DLL path (UTF-16) to target process...");
        let mut bytes_written = 0;
        let write_result = WriteProcessMemory(
            process,
            remote_memory,
            wide_path.as_ptr() as *const _,
            path_len,
            Some(&mut bytes_written),
        );

        if write_result.is_err() {
            let msg = "[INJECT] ERROR: WriteProcessMemory failed".to_string();
            println!("{}", msg);
            log_to_file(&msg);
            let _ = VirtualFreeEx(process, remote_memory, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("Failed to write DLL path to target process".to_string());
        }
        let msg = format!("[INJECT] Wrote {} bytes to target process", bytes_written);
        println!("{}", msg);
        log_to_file(&msg);

        // Create remote thread to load the DLL
        log_to_file("[INJECT] Creating remote thread...");
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
                let msg = format!("[INJECT] Remote thread created: {:?}", handle);
                println!("{}", msg);
                log_to_file(&msg);

                log_to_file("[INJECT] Waiting for thread to complete (5s timeout)...");
                println!("[INJECT] Waiting for thread to complete (5s timeout)...");
                let wait_result = windows::Win32::System::Threading::WaitForSingleObject(handle, 5000);
                let msg = format!("[INJECT] WaitForSingleObject result: {:?}", wait_result);
                println!("{}", msg);
                log_to_file(&msg);

                // Check the return value of LoadLibraryW
                let mut exit_code: u32 = 0;
                if GetExitCodeThread(handle, &mut exit_code).is_ok() {
                    let msg = format!("[INJECT] LoadLibraryW returned: 0x{:08x}", exit_code);
                    println!("{}", msg);
                    log_to_file(&msg);

                    if exit_code == 0 {
                        log_to_file("[INJECT] ERROR: LoadLibraryW FAILED! DLL did not load.");
                        log_to_file("[INJECT] Possible causes:");
                        log_to_file("[INJECT]   - DLL has missing dependencies");
                        log_to_file("[INJECT]   - DLL architecture mismatch");
                        println!("[INJECT] ERROR: LoadLibraryW FAILED! DLL did not load.");
                        println!("[INJECT] Possible causes:");
                        println!("[INJECT]   - DLL has missing dependencies");
                        println!("[INJECT]   - DLL architecture mismatch");

                        let _ = CloseHandle(handle);
                        let _ = VirtualFreeEx(process, remote_memory, 0, MEM_RELEASE);
                        let _ = CloseHandle(process);
                        return Err(format!("LoadLibraryA failed - DLL exists but could not be loaded. Check dependencies and architecture."));
                    } else {
                        let msg = format!("[INJECT] SUCCESS: DLL loaded at base address 0x{:08x}", exit_code);
                        println!("{}", msg);
                        log_to_file(&msg);
                    }
                }

                let _ = CloseHandle(handle);
            }
            Err(e) => {
                let msg = format!("[INJECT] ERROR: CreateRemoteThread failed: {}", e);
                println!("{}", msg);
                log_to_file(&msg);
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
    log_to_file("[GET_DLL_PATH] Searching for DLL...");

    // Get the directory where the executable is located
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(dir) = &exe_dir {
        let msg = format!("[GET_DLL_PATH] Executable directory: {:?}", dir);
        println!("{}", msg);
        log_to_file(&msg);
    } else {
        log_to_file("[GET_DLL_PATH] ERROR: Could not get executable directory");
    }

    // Try to find the DLL in common locations
    // Note: Use ASCII-only paths to avoid LoadLibraryA encoding issues
    let mut possible_paths: Vec<String> = Vec::new();

    // First, check next to the executable
    if let Some(dir) = exe_dir {
        let dll_next_to_exe = dir.join("sc_hook_dll.dll");
        possible_paths.push(dll_next_to_exe.to_string_lossy().to_string());
    }

    // Fallback paths
    possible_paths.push(r"C:\temp\sc_hook_dll.dll".to_string());
    possible_paths.push("sc_hook_dll.dll".to_string());

    for path in &possible_paths {
        let msg = format!("[GET_DLL_PATH] Checking: {}", path);
        println!("{}", msg);
        log_to_file(&msg);

        if std::path::Path::new(path).exists() {
            let msg = format!("[GET_DLL_PATH] FOUND: {}", path);
            println!("{}", msg);
            log_to_file(&msg);
            return path.clone();
        } else {
            log_to_file("[GET_DLL_PATH] Not found at this path");
            println!("[GET_DLL_PATH] Not found at this path");
        }
    }

    let msg = format!("[GET_DLL_PATH] No DLL found, returning default: {}", possible_paths[0]);
    println!("{}", msg);
    log_to_file(&msg);
    possible_paths[0].clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Generate blacklist_targets.txt from existing blacklist.json on startup
    let entries = load_blacklist_from_file();
    if !entries.is_empty() {
        write_blacklist_targets_file(&entries);
    }

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
            is_dll_injected,
            find_starcraft_process,
            inject_dll,
            get_default_dll_path,
            get_room_history,
            add_room_history,
            clear_room_history,
            remove_room_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
